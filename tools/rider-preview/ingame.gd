extends SceneTree

## In-game pictures and timings of the rider. `ingame.sh` runs this.
##
## Run with the game project as `--path` and this file's absolute path as
## `--script`. It loads `res://main.tscn`, the way `godot/tests/shot.gd`
## does, then drives the ride through the game's `debug_*` methods.
##
## User args (after --):
##   --out-dir DIR   where the PNG files go
##   --repo DIR      repository root, for the kept picture of the placeholder
##   --only a,b,c    jobs to run: stills, compare, sequence, contact, scale,
##                   bend, stand, tuck, firstperson, eye, shadow, ridestate.
##                   Default: all
##   --bench         print timings at the switchback instead of pictures
##   --frames N      frames per benchmark phase (default 400)
##   --preset ID     which rider to show (default nils)
##   --prefix TEXT   prepended to each PNG name
##
## This script never opens Bluetooth. `ingame.sh` passes `--no-bluetooth`, a
## scratch settings file, and `--dashboard-port 0`; when a flag is missing
## the same switches are set in the environment before the scene loads.

const SIZE := Vector2i(1280, 800)
const PLACES := {
	"start": 8.0,
	"switchback": 340.0,
	"climb": 900.0,
	"summit": 2500.0,
	"descent": 3800.0,
}
## Frames drawn after a pose changes, before the picture is taken.
const SETTLE_FRAMES := 4

var _out_dir := ""
var _repo := ""
var _only: PackedStringArray = []
var _bench := false
var _frames := 400
var _preset := ""
var _prefix := ""

var _viewport: SubViewport
var _game: Node
var _rider: Node3D
var _hud: CanvasLayer
var _game_camera: Camera3D
var _free_camera: Camera3D
var _seen := 0
var _started := false

var _jobs: Array = []
var _job := 0
var _tile := 0
var _settle := 0
var _images: Array[Image] = []
var _road_before: Mesh

var _phases: Array = []
var _phase := 0
var _phase_seen := 0
var _last_usec := 0
var _dt: Array[float] = []
var _cpu: Array[float] = []
var _gpu: Array[float] = []


func _init() -> void:
	var args := OS.get_cmdline_user_args()
	_out_dir = _arg(args, "--out-dir", "")
	_repo = _arg(args, "--repo", "")
	var only := _arg(args, "--only", "")
	if not only.is_empty():
		_only = only.split(",", false)
	_bench = args.has("--bench")
	_frames = int(_arg(args, "--frames", str(_frames)))
	_preset = _arg(args, "--preset", "")
	_prefix = _arg(args, "--prefix", "")
	if _out_dir.is_empty() and not _bench:
		push_error("--out-dir is required")
		quit(2)
		return
	if not args.has("--fake-trainer") and not args.has("--no-bluetooth"):
		OS.set_environment("PENGUIN_PASS_NO_BLUETOOTH", "1")
	if not args.has("--dashboard-port"):
		OS.set_environment("PENGUIN_PASS_DASHBOARD_PORT", "0")
		OS.set_environment("PENGUIN_PASS_DASHBOARD_LOOPBACK", "1")
	if not args.has("--settings"):
		var settings_path := "/tmp/penguin-pass-rider-%d.toml" % OS.get_process_id()
		var settings_file := FileAccess.open(settings_path, FileAccess.WRITE)
		if settings_file != null:
			settings_file.store_string("fullscreen = false\ndashboard_enabled = false\n")
			settings_file.close()
		OS.set_environment("PENGUIN_PASS_SETTINGS", settings_path)

	_viewport = SubViewport.new()
	_viewport.name = "ShotViewport"
	_viewport.size = SIZE
	_viewport.own_world_3d = true
	_viewport.transparent_bg = false
	_viewport.render_target_update_mode = SubViewport.UPDATE_ALWAYS
	_viewport.msaa_3d = SubViewport.MSAA_2X
	_viewport.handle_input_locally = true
	root.add_child(_viewport)
	var packed := load("res://main.tscn") as PackedScene
	if packed == null:
		push_error("main.tscn failed to load")
		quit(1)
		return
	_game = packed.instantiate()
	_viewport.add_child(_game)
	RenderingServer.frame_post_draw.connect(_on_drawn)


func _arg(args: PackedStringArray, name: String, fallback: String) -> String:
	var index := args.find(name)
	if index >= 0 and index + 1 < args.size():
		return args[index + 1]
	return fallback


func _wanted(name: String) -> bool:
	return _only.is_empty() or _only.has(name)


func _release() -> void:
	if RenderingServer.frame_post_draw.is_connected(_on_drawn):
		RenderingServer.frame_post_draw.disconnect(_on_drawn)
	if _game != null and is_instance_valid(_game):
		_game.free()
		_game = null
	if _viewport != null and is_instance_valid(_viewport):
		_viewport.free()
		_viewport = null


func _on_drawn() -> void:
	_seen += 1
	# SceneTree._init adds the scene before `_ready`. Wait until it has run.
	if _seen < 3:
		return
	if not _started:
		_started = true
		if not _start():
			_release()
			quit(1)
			return
	if _bench:
		_bench_frame()
	else:
		_picture_frame()


func _start() -> bool:
	_rider = _game.get_node_or_null("Rider") as Node3D
	_hud = _game.get_node_or_null("Hud") as CanvasLayer
	_game_camera = _game.get_node_or_null("CameraRig/Camera") as Camera3D
	if _rider == null or _game_camera == null:
		push_error("the game has no Rider or no CameraRig/Camera")
		return false
	if not _preset.is_empty():
		if not _rider.has_method("set_preset"):
			push_error("Rider has no set_preset")
			return false
		if not bool(_rider.call("set_preset", _preset)):
			push_error("unknown rider preset %s" % _preset)
			return false
	# The game opens on the title and holds the rider at the start line.
	# Pictures are of the ride.
	if _game.has_method("debug_start_ride"):
		_game.debug_start_ride()
	_free_camera = Camera3D.new()
	_free_camera.name = "RiderPreviewCamera"
	_free_camera.near = 0.05
	_free_camera.far = 8000.0
	_viewport.add_child(_free_camera)
	if _bench:
		DisplayServer.window_set_vsync_mode(DisplayServer.VSYNC_DISABLED)
		RenderingServer.viewport_set_measure_render_time(_viewport.get_viewport_rid(), true)
		_phases = ["shown", "hidden", "shown", "hidden"]
		_begin_phase()
		return true
	# The pictures drive the ride themselves, so the same command gives the
	# same frames. `debug_advance` steps and presents without `_process`.
	_game.set_process(false)
	if _wanted("ridestate") and not _check_ride_state():
		return false
	_jobs = _build_jobs()
	if _jobs.is_empty():
		push_error("no jobs to render")
		return false
	_begin_tile()
	return true


# ---------------------------------------------------------------- pictures

func _still(place: String, first: bool) -> Dictionary:
	return {
		"name": "ingame-%s-%s" % [place, "fp" if first else "chase"],
		"cols": 1,
		"tiles": [{"place": place, "camera": "first" if first else "chase", "hud": true}],
	}


func _free(place: String, pos: Vector3, look: Vector3, fov: float) -> Dictionary:
	return {"place": place, "camera": "free", "pos": pos, "look": look, "fov": fov}


func _build_jobs() -> Array:
	var jobs: Array = []
	if _wanted("stills"):
		for place in PLACES:
			jobs.append(_still(place, false))
			jobs.append(_still(place, true))

	if _wanted("compare") and not _repo.is_empty():
		# The placeholder rider at the switchback, photographed in this
		# world before the model went in, beside the model there now. The
		# placeholder shot had ridden on about two seconds past the seek.
		var before := {
			"place": "switchback", "camera": "chase",
			"image": _repo.path_join("docs/img/rider/history/ingame-before/switchback-chase.png"),
		}
		var after := {"place": "switchback", "camera": "chase", "settle_s": 2.2, "hud": true}
		jobs.append({"name": "ingame-compare", "cols": 2, "tiles": [before, after]})

	if _wanted("sequence"):
		# Twelve frames across one pedal revolution at 85 rpm, cropped to
		# the rider. Top sheet: the chase camera. Bottom sheet: side on.
		var step := 60.0 / 85.0 / 12.0
		var chase: Array = []
		var side: Array = []
		for i in 12:
			var first := i == 0
			chase.append({
				"place": "climb", "camera": "chase", "seek": first,
				"advance": 0.0 if first else step,
				"crop": Rect2i(280, 170, 560, 560),
			})
			var tile := _free("climb", Vector3(4.2, 0.95, -0.1), Vector3(0, 0.85, -0.1), 30.0)
			tile["seek"] = first
			tile["advance"] = 0.0 if first else step
			tile["crop"] = Rect2i(280, 40, 720, 720)
			tile["scale"] = Vector2i(560, 560)
			side.append(tile)
		jobs.append({"name": "ingame-pedal-sequence", "cols": 6, "tiles": chase})
		jobs.append({"name": "ingame-pedal-sequence-side", "cols": 6, "tiles": side})

	if _wanted("contact"):
		# Where the tyres meet the road, one row each for the climb, the
		# descent, and the switchback: side on, then the rear and the front
		# contact patch from road level (each about 25 cm of road tall).
		var tiles: Array = []
		for place in ["climb", "descent", "switchback"]:
			# The hillside is close on the right of the descent.
			var side := -1.0 if place == "descent" else 1.0
			tiles.append(_free(place, Vector3(side * 3.4, 0.7, -0.1), Vector3(0, 0.62, -0.1), 34.0))
			tiles.append(_free(place, Vector3(side * 1.2, 0.03, 0.404), Vector3(0, 0.03, 0.404), 12.0))
			tiles.append(_free(place, Vector3(side * 1.2, 0.03, -0.588), Vector3(0, 0.03, -0.588), 12.0))
		jobs.append({"name": "ingame-contact", "cols": 3, "tiles": tiles, "scale": Vector2i(960, 600)})

	if _wanted("scale"):
		# The rider against the road, the rails, and the trees.
		var tiles: Array = [
			_free("switchback", Vector3(-2.5, 2.2, -7.5), Vector3(0, 0.9, 0), 50.0),
			_free("switchback", Vector3(4.5, 1.6, 5.5), Vector3(0, 0.9, 0), 50.0),
			_free("climb", Vector3(3.2, 1.5, -6.5), Vector3(0, 0.9, 0), 50.0),
			_free("start", Vector3(0.0, 9.0, 2.5), Vector3(0, 0.0, -2.0), 55.0),
		]
		jobs.append({"name": "ingame-scale", "cols": 2, "tiles": tiles, "scale": Vector2i(960, 600)})

	if _wanted("bend"):
		# Lean and steering in the switchback, from ahead and from behind.
		var tiles: Array = [
			_free("switchback", Vector3(0.0, 1.0, -4.5), Vector3(0, 0.85, 0), 34.0),
			_free("switchback", Vector3(0.0, 2.6, 2.4), Vector3(0, 0.6, -0.8), 40.0),
		]
		jobs.append({"name": "ingame-bend", "cols": 2, "tiles": tiles, "scale": Vector2i(960, 600)})

	if _wanted("stand"):
		# Out of the saddle: a steep stretch at 55 rpm. Six frames of one
		# revolution from the chase camera, then side on.
		var step := 60.0 / 55.0 / 6.0
		var tiles: Array = []
		for i in 6:
			tiles.append({
				"place": "climb", "camera": "chase", "seek": i == 0, "cadence": 55.0,
				"settle_s": 2.5, "advance": 0.0 if i == 0 else step,
				"crop": Rect2i(280, 170, 560, 560),
			})
		for i in 6:
			var tile := _free("climb", Vector3(4.2, 1.0, -0.1), Vector3(0, 0.9, -0.1), 30.0)
			tile["seek"] = false
			tile["cadence"] = 55.0
			tile["advance"] = step
			tile["crop"] = Rect2i(280, 40, 720, 720)
			tile["scale"] = Vector2i(560, 560)
			tiles.append(tile)
		jobs.append({"name": "ingame-stand", "cols": 6, "tiles": tiles})

	if _wanted("tuck"):
		# Coasting down the fast descent: chase, side on, and the handlebar
		# camera at its rest position (see the first-person job).
		var chase := {"place": "descent", "camera": "chase", "coast": true, "settle_s": 3.0, "hud": true}
		var side := _free("descent", Vector3(4.2, 0.9, -0.1), Vector3(0, 0.8, -0.1), 30.0)
		side["seek"] = false
		side["coast"] = true
		var first := {"place": "descent", "camera": "first", "rigid": true, "seek": false, "coast": true, "hud": true}
		jobs.append({"name": "ingame-tuck", "cols": 3, "tiles": [chase, side, first], "scale": Vector2i(960, 600)})

	if _wanted("firstperson"):
		# The handlebar camera in each posture: seated, leaning in the
		# switchback, out of the saddle, and tucked. In play the game's
		# camera trails the bike by its speed over 7.5 per second, which
		# puts it behind the rider's back (the `-fp` stills show that).
		# These tiles cut to the camera's rest position first, which is
		# what a camera fixed to the bike would show.
		var tiles: Array = [
			{"place": "start", "camera": "first", "rigid": true, "hud": true},
			{"place": "switchback", "camera": "first", "rigid": true, "hud": true},
			{"place": "climb", "camera": "first", "rigid": true, "cadence": 55.0, "settle_s": 2.5, "hud": true},
			{"place": "descent", "camera": "first", "rigid": true, "coast": true, "settle_s": 3.0, "hud": true},
		]
		jobs.append({"name": "ingame-first-person-rigid", "cols": 2, "tiles": tiles, "scale": Vector2i(960, 600)})

	if _wanted("eye"):
		# What a camera at the rider's eyes would see with the head hidden.
		var tiles: Array = []
		for place in ["start", "switchback", "descent"]:
			tiles.append({"place": place, "camera": "eye", "hud": true})
		jobs.append({"name": "ingame-eye", "cols": 3, "tiles": tiles, "scale": Vector2i(960, 600)})

	if _wanted("shadow"):
		# The ribbon's top is the front face, so the rider's shadow falls
		# on the asphalt. One tile, the game as it is.
		var as_is := _free("start", Vector3(-2.6, 2.4, -4.2), Vector3(0, 0.6, 0), 40.0)
		jobs.append({"name": "ingame-shadow-check", "cols": 1, "tiles": [as_is], "scale": Vector2i(960, 600)})
	return jobs


## Rewind the road ribbon so its top side is the front face. Returns the
## mesh that was there, to put back afterwards.
func _flip_road() -> Mesh:
	var road := _game.get_node_or_null("World/Road") as MeshInstance3D
	if road == null or road.mesh == null:
		push_error("the game has no World/Road mesh")
		return null
	var original := road.mesh
	var arrays := original.surface_get_arrays(0)
	var indices := PackedInt32Array()
	if arrays[Mesh.ARRAY_INDEX] == null:
		var count: int = (arrays[Mesh.ARRAY_VERTEX] as PackedVector3Array).size()
		for i in range(0, count, 3):
			indices.append_array([i, i + 2, i + 1])
	else:
		var old: PackedInt32Array = arrays[Mesh.ARRAY_INDEX]
		for i in range(0, old.size(), 3):
			indices.append_array([old[i], old[i + 2], old[i + 1]])
	arrays[Mesh.ARRAY_INDEX] = indices
	var flipped := ArrayMesh.new()
	flipped.add_surface_from_arrays(Mesh.PRIMITIVE_TRIANGLES, arrays)
	road.mesh = flipped
	return original


## `Rider.set_ride_state` is the hookup `Game` does not make yet. Call it
## from here with a ride that contradicts the road under the wheels, and
## check that the next cut follows it and the one after forgets it.
func _check_ride_state() -> bool:
	if not _rider.has_method("set_ride_state"):
		push_error("the Rider has no set_ride_state()")
		return false
	var failures: PackedStringArray = []
	_game.debug_seek(float(PLACES["start"]), 250.0, 0.0)
	_game.debug_advance(1.0)

	# Pedalling on the flat start, told it is a hard effort.
	_rider.set_ride_state(300.0, 200.0, 0.0, 5.0)
	_game.debug_set_first_person(false)
	if absf(float(_rider.effort()) - 1.0) > 0.01:
		failures.append("effort %.2f after 300 W at 200 W FTP" % float(_rider.effort()))
	_game.debug_set_first_person(false)
	if float(_rider.effort()) > 0.5:
		failures.append("effort %.2f once the ride state was used up" % float(_rider.effort()))

	# Told it is grinding up a 10 % climb.
	_rider.debug_override_cadence(55.0)
	_rider.set_ride_state(250.0, 200.0, 10.0, 3.0)
	_game.debug_set_first_person(false)
	if float(_rider.stand_blend()) < 0.99:
		failures.append("stand %.2f on a stated 10 %% grade at 55 rpm" % float(_rider.stand_blend()))
	_game.debug_set_first_person(false)
	if float(_rider.stand_blend()) > 0.01:
		failures.append("stand %.2f back on the flat" % float(_rider.stand_blend()))
	_rider.debug_override_cadence(-1.0)

	# Coasting, told it is a fast descent.
	_game.debug_set_target_power(0.0)
	_game.debug_advance(0.1)
	_rider.set_ride_state(0.0, 200.0, -9.0, 20.0)
	_game.debug_set_first_person(false)
	if float(_rider.tuck_blend()) < 0.99:
		failures.append("tuck %.2f on a stated -9 %% grade at 72 km/h" % float(_rider.tuck_blend()))
	# The stated speed is all a cut has to go on. Riding on, the rider's
	# own estimate takes over and they sit up.
	_game.debug_advance(3.0)
	if float(_rider.tuck_blend()) > 0.01:
		failures.append("tuck %.2f after three seconds back on the flat" % float(_rider.tuck_blend()))

	for failure in failures:
		push_error("ride state: " + failure)
	if failures.is_empty():
		print("RIDER_INGAME_RIDE_STATE ok: effort, stand, and tuck follow set_ride_state for one frame")
	return failures.is_empty()


func _begin_tile() -> void:
	var job: Dictionary = _jobs[_job]
	var tile: Dictionary = job["tiles"][_tile]
	if tile.has("image"):
		_settle = 0
		return
	var place: String = tile["place"]
	var coast: bool = tile.get("coast", false)
	if _rider.has_method("debug_override_cadence"):
		_rider.debug_override_cadence(float(tile.get("cadence", -1.0)))
	var camera: String = tile["camera"]
	var first := camera == "first"
	var seek: bool = tile.get("seek", true)
	if seek:
		_game.debug_seek(float(PLACES[place]), 250.0, 0.0)
		if coast:
			_game.debug_set_target_power(0.0)
	# Seeking and switching the camera are cuts: the chase camera snaps to
	# its rest position and the rider's pose jumps. Ride on afterwards, so
	# the picture has the camera lag and the smoothing that play has.
	if first != bool(_game.camera_is_first_person()):
		_game.debug_set_first_person(first)
		if not seek:
			_game.debug_advance(1.0)
	if seek:
		_game.debug_advance(float(tile.get("settle_s", 1.0)))
	var advance: float = tile.get("advance", 0.0)
	if advance > 0.0:
		_game.debug_advance(advance)
	if tile.get("rigid", false):
		# Presenting again with no time passed is a cut.
		_game.debug_set_first_person(first)
	if _hud != null:
		_hud.visible = tile.get("hud", false)
	if tile.get("flip_road", false):
		_road_before = _flip_road()
	var at := _rider.global_transform
	if camera == "free":
		var pos: Vector3 = tile["pos"]
		var look: Vector3 = tile["look"]
		_free_camera.fov = tile["fov"]
		_free_camera.look_at_from_position(at * pos, at * look, Vector3.UP)
		_free_camera.current = true
	elif camera == "eye":
		if not _rider.has_method("eye_position"):
			push_error("the Rider has no eye_position()")
			quit(1)
			return
		var eye: Vector3 = _rider.eye_position()
		_free_camera.fov = 72.0
		_free_camera.near = 0.08
		_free_camera.look_at_from_position(eye, at * Vector3(0.0, 0.55, -6.0), Vector3.UP)
		_free_camera.current = true
	else:
		_game_camera.current = true
	var line := "RIDER_INGAME_TILE %s[%d] %s %s d=%.1f grade=%.1f%% speed=%.1fkm/h" % [
		job["name"], _tile, place, camera,
		float(_game.distance_m()), float(_game.grade_pct()), float(_game.speed_kph()),
	]
	if _rider.has_method("stand_blend"):
		line += " stand=%.2f tuck=%.2f steer=%.3f effort=%.2f" % [
			float(_rider.stand_blend()), float(_rider.tuck_blend()),
			float(_rider.steer_angle()), float(_rider.effort()),
		]
	if _rider.has_method("eye_position"):
		var lens := _viewport.get_camera_3d().global_position
		line += " camera-to-eye=%.2fm" % lens.distance_to(_rider.eye_position())
	print(line)
	_settle = 0


func _picture_frame() -> void:
	_settle += 1
	if _settle < SETTLE_FRAMES:
		return
	var job: Dictionary = _jobs[_job]
	var tile: Dictionary = job["tiles"][_tile]
	var image: Image
	if tile.has("image"):
		image = Image.load_from_file(tile["image"])
	else:
		image = _viewport.get_texture().get_image()
	if image == null or image.get_width() == 0:
		push_error("no picture for %s[%d]" % [job["name"], _tile])
		_release()
		quit(1)
		return
	var head := _rider.get_node_or_null("head") as MeshInstance3D
	if head != null and head.cast_shadow == GeometryInstance3D.SHADOW_CASTING_SETTING_SHADOWS_ONLY:
		print("RIDER_INGAME_HEAD %s[%d] drawn as a shadow only" % [job["name"], _tile])
	image.convert(Image.FORMAT_RGBA8)
	if tile.has("crop"):
		image = image.get_region(tile["crop"])
	var scale: Vector2i = tile.get("scale", job.get("scale", Vector2i.ZERO))
	if scale != Vector2i.ZERO:
		image.resize(scale.x, scale.y, Image.INTERPOLATE_LANCZOS)
	_images.append(image)
	if _road_before != null:
		(_game.get_node("World/Road") as MeshInstance3D).mesh = _road_before
		_road_before = null
	_tile += 1
	if _tile < job["tiles"].size():
		_begin_tile()
		return
	if not _save_sheet(job):
		_release()
		quit(1)
		return
	_images.clear()
	_tile = 0
	_job += 1
	if _job < _jobs.size():
		_begin_tile()
		return
	print("RIDER_INGAME_DONE")
	_release()
	quit(0)


func _save_sheet(job: Dictionary) -> bool:
	var cols: int = job["cols"]
	var rows := int(ceil(float(_images.size()) / float(cols)))
	var tile_size := _images[0].get_size()
	var gap := 0 if _images.size() == 1 else 8
	var sheet := Image.create(
		cols * tile_size.x + (cols - 1) * gap,
		rows * tile_size.y + (rows - 1) * gap,
		false, Image.FORMAT_RGBA8)
	sheet.fill(Color(0.1, 0.1, 0.1))
	for index in _images.size():
		var at := Vector2i(
			(index % cols) * (tile_size.x + gap),
			(index / cols) * (tile_size.y + gap))
		sheet.blit_rect(_images[index], Rect2i(Vector2i.ZERO, tile_size), at)
	DirAccess.make_dir_recursive_absolute(_out_dir)
	var path := _out_dir.path_join(_prefix + str(job["name"]) + ".png")
	var err := sheet.save_png(path)
	if err != OK:
		push_error("save_png failed: %s" % err)
		return false
	print("RIDER_INGAME %s %dx%d" % [path, sheet.get_width(), sheet.get_height()])
	return true


# --------------------------------------------------------------- benchmark

func _begin_phase() -> void:
	_game.debug_seek(float(PLACES["switchback"]), 250.0, 0.0)
	_game.debug_set_first_person(false)
	_rider.visible = _phases[_phase] == "shown"
	_phase_seen = 0
	_dt.clear()
	_cpu.clear()
	_gpu.clear()
	_last_usec = Time.get_ticks_usec()


func _bench_frame() -> void:
	var now := Time.get_ticks_usec()
	var dt_ms := float(now - _last_usec) / 1000.0
	_last_usec = now
	_phase_seen += 1
	if _phase_seen <= 60:
		return
	var rid := _viewport.get_viewport_rid()
	_dt.append(dt_ms)
	_cpu.append(RenderingServer.viewport_get_measured_render_time_cpu(rid))
	_gpu.append(RenderingServer.viewport_get_measured_render_time_gpu(rid))
	if _dt.size() < _frames:
		return
	var visible_type := RenderingServer.VIEWPORT_RENDER_INFO_TYPE_VISIBLE
	var shadow_type := RenderingServer.VIEWPORT_RENDER_INFO_TYPE_SHADOW
	var draws := RenderingServer.VIEWPORT_RENDER_INFO_DRAW_CALLS_IN_FRAME
	var prims := RenderingServer.VIEWPORT_RENDER_INFO_PRIMITIVES_IN_FRAME
	print(
		"RIDER_BENCH rider=%s n=%d frame_ms avg=%.3f p95=%.3f max=%.3f | render cpu_ms avg=%.3f gpu_ms avg=%.3f | draw calls %d + %d shadow, primitives %d + %d shadow"
		% [
			_phases[_phase], _dt.size(), _avg(_dt), _p95(_dt), _dt.max(),
			_avg(_cpu), _avg(_gpu),
			RenderingServer.viewport_get_render_info(rid, visible_type, draws),
			RenderingServer.viewport_get_render_info(rid, shadow_type, draws),
			RenderingServer.viewport_get_render_info(rid, visible_type, prims),
			RenderingServer.viewport_get_render_info(rid, shadow_type, prims),
		]
	)
	_phase += 1
	if _phase < _phases.size():
		_begin_phase()
		return
	print("RIDER_INGAME_DONE")
	_release()
	quit(0)


func _avg(values: Array[float]) -> float:
	var total := 0.0
	for value in values:
		total += value
	return total / float(max(values.size(), 1))


func _p95(values: Array[float]) -> float:
	var copy := values.duplicate()
	copy.sort()
	return copy[int(float(copy.size() - 1) * 0.95)]
