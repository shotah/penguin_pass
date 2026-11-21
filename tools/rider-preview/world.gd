extends SceneTree

## Build times and pictures for the world slice. `ingame.sh world` runs this.
##
## Same safe launch as ingame.sh: --no-bluetooth, a scratch settings file,
## --dashboard-port 0. It never opens Bluetooth.

const SIZE := Vector2i(1280, 800)

var _out := ""
var _rider_dir := ""
var _viewport: SubViewport
var _game: Node
var _rider: Node3D
var _camera: Camera3D
var _free: Camera3D
var _seen := 0
var _wait := 0
var _shot_path := ""
var _band := false
var _queue: Array = []


func _init() -> void:
	var args := OS.get_cmdline_user_args()
	_out = _arg(args, "--out-dir", "")
	_rider_dir = _arg(args, "--rider-dir", "")
	if _out.is_empty():
		push_error("--out-dir is required")
		quit(2)
		return
	DirAccess.make_dir_recursive_absolute(_out)
	if not _rider_dir.is_empty():
		DirAccess.make_dir_recursive_absolute(_rider_dir)
	if not args.has("--fake-trainer") and not args.has("--no-bluetooth"):
		OS.set_environment("PENGUIN_PASS_NO_BLUETOOTH", "1")
	if not args.has("--dashboard-port"):
		OS.set_environment("PENGUIN_PASS_DASHBOARD_PORT", "0")
		OS.set_environment("PENGUIN_PASS_DASHBOARD_LOOPBACK", "1")
	_viewport = SubViewport.new()
	_viewport.size = SIZE
	_viewport.own_world_3d = true
	_viewport.render_target_update_mode = SubViewport.UPDATE_ALWAYS
	_viewport.msaa_3d = SubViewport.MSAA_2X
	root.add_child(_viewport)
	var packed := load("res://main.tscn") as PackedScene
	_game = packed.instantiate()
	_viewport.add_child(_game)
	RenderingServer.frame_post_draw.connect(_on_drawn)


func _arg(args: PackedStringArray, name: String, fallback: String) -> String:
	var index := args.find(name)
	if index >= 0 and index + 1 < args.size():
		return args[index + 1]
	return fallback


func _on_drawn() -> void:
	_seen += 1
	if _seen < 3:
		return
	if _shot_path != "":
		_save_shot()
		_shot_path = ""
		_band = false
		return
	if _wait > 0:
		_wait -= 1
		return
	if _queue.is_empty():
		if _rider == null:
			_setup()
		else:
			_finish()
		return
	var job: Callable = _queue.pop_front()
	job.call()


func _setup() -> void:
	if _game.has_method("debug_start_ride"):
		_game.debug_start_ride()
	_rider = _game.get_node("Rider") as Node3D
	_camera = _game.get_node("CameraRig/Camera") as Camera3D
	_free = Camera3D.new()
	_free.near = 0.08
	_free.far = 8000.0
	_viewport.add_child(_free)
	_count("builtin-5km")
	_queue.append(func() -> void:
		_show_ui()
		_game.debug_set_first_person(false)
		_game.debug_seek(900.0, 250.0, 0.0)
		_game.debug_advance(2.0)
		_camera.current = true
		_arm_shot(_rider_dir.path_join("framing-1280.png"), true)
	)
	_queue.append(func() -> void:
		_viewport.size = Vector2i(1920, 1080)
		_wait = 2
	)
	_queue.append(func() -> void:
		_show_ui()
		_camera.current = true
		_arm_shot(_rider_dir.path_join("framing-1080.png"), true)
	)
	_queue.append(func() -> void:
		_viewport.size = SIZE
		_wait = 2
	)
	_queue_fp(17.0, 140.0)
	_queue_fp(30.0, 200.0)
	_queue_fp(79.0, 250.0)
	_queue.append(func() -> void:
		_show_ui()
		_game.debug_set_first_person(false)
		_game.debug_seek(3800.0, 250.0, 0.0)
		_game.debug_advance(2.5)
		_camera.current = true
		print("WORLD_DESCENT speed=%.1f" % float(_game.speed_kph()))
		_arm_shot(_rider_dir.path_join("descent-chase.png"), true)
	)
	_queue.append(func() -> void:
		_hide_ui()
		_game.debug_set_first_person(false)
		_game.debug_seek(40.0, 200.0, 0.0)
		_game.debug_advance(1.0)
		var at := _rider.global_transform
		_free.fov = 40.0
		_free.look_at_from_position(at * Vector3(-2.6, 2.4, -4.2), at * Vector3(0, 0.6, 0), Vector3.UP)
		_free.current = true
		_arm_shot(_rider_dir.path_join("shadow-road.png"), false)
	)
	_queue.append(func() -> void:
		_hide_ui()
		_game.debug_set_first_person(false)
		_game.debug_seek(340.0, 220.0, 0.0)
		_game.debug_advance(0.4)
		_look_down()
		_arm_shot(_out.path_join("builtin-above.png"), false)
	)
	_queue.append(func() -> void:
		_game.debug_set_first_person(true)
		_game.debug_advance(1.0)
		_camera.current = true
		_arm_shot(_out.path_join("builtin-saddle.png"), false)
	)
	_queue_course("rolling", 5, 3, 400.0, false)
	_queue_course("mountain", 10, 11, 0.0, true)
	_queue_course("desert", 10, 7, 700.0, false)
	_queue.append(func() -> void:
		_hide_ui()
		_game.debug_set_first_person(false)
		_game.debug_seek(1600.0, 200.0, 0.0)
		_game.debug_advance(1.2)
		_camera.current = true
		_arm_shot(_out.path_join("desert-chase.png"), false)
	)
	_queue.append(func() -> void:
		_hide_ui()
		_game.debug_set_first_person(false)
		_game.debug_seek(1400.0, 200.0, 0.0)
		_game.debug_advance(1.0)
		var at := _rider.global_transform
		var side := at.basis.x.normalized()
		_free.fov = 55.0
		_free.look_at_from_position(
			at.origin + Vector3(0, 1.7, 0),
			at.origin + side * 240.0 + Vector3(0, 6.0, 0),
			Vector3.UP
		)
		_free.current = true
		_arm_shot(_out.path_join("corridor-edge.png"), false)
	)
	_queue_course("flat", 10, 5, 500.0, false)
	_queue.append(func() -> void:
		# Back to the built-in loop the way the course list does, then quit.
		if _game.has_method("debug_menu_action"):
			_game.debug_menu_action("back")
			_game.debug_menu_action("down")
			_game.debug_menu_action("accept")
			if str(_game.debug_screen()) == "course":
				_game.debug_menu_action("accept")
		_count("back")
	)


func _queue_fp(target: float, power: float) -> void:
	_queue.append(func() -> void:
		var found := _nearest_speed(target, power)
		_game.debug_set_first_person(true)
		_game.debug_seek(found, power, 0.0)
		_game.debug_advance(1.5)
		var eye := _camera.global_position.distance_to(_rider.eye_position())
		print(
			"WORLD_FP target=%.0f speed=%.1f distance=%.0f camera-to-eye=%.2f"
			% [target, float(_game.speed_kph()), found, eye]
		)
		_show_ui()
		_camera.current = true
		_arm_shot(_rider_dir.path_join("fp-%d.png" % int(target)), false)
	)


func _queue_course(terrain: String, km: int, seed: int, distance: float, hairpin: bool) -> void:
	_queue.append(func() -> void:
		var started := Time.get_ticks_msec()
		var ok: bool = _game.debug_load_course(terrain, km, seed)
		print(
			"WORLD_LOAD %s %dkm seed %d ok=%s ms=%d"
			% [terrain, km, seed, ok, Time.get_ticks_msec() - started]
		)
		if not ok:
			return
		_count("%s-%dkm-seed%d" % [terrain, km, seed])
		_hide_ui()
		_game.debug_set_first_person(false)
		var at := distance
		if hairpin:
			at = _hairpin_distance()
		_game.debug_seek(at, 220.0, 0.0)
		_game.debug_advance(0.4)
		_look_down()
		_arm_shot(_out.path_join("%s-above.png" % terrain), false)
	)
	_queue.append(func() -> void:
		_hide_ui()
		_game.debug_set_first_person(true)
		_game.debug_advance(1.0)
		_camera.current = true
		_arm_shot(_out.path_join("%s-saddle.png" % terrain), false)
	)


func _nearest_speed(target: float, power: float) -> float:
	var best := 200.0
	var best_err := 1000.0
	var distance := 80.0
	while distance < 4800.0:
		_game.debug_seek(distance, power, 0.0)
		_game.debug_advance(0.8)
		var err := absf(float(_game.speed_kph()) - target)
		if err < best_err:
			best_err = err
			best = distance
		distance += 320.0
	return best


func _hairpin_distance() -> float:
	var best := 1500.0
	var best_turn := 0.0
	var distance := 400.0
	while distance < 9200.0:
		_game.debug_seek(distance, 200.0, 0.0)
		_game.debug_advance(0.2)
		var before := _rider.global_transform.basis.z
		_game.debug_seek(distance + 35.0, 200.0, 0.0)
		_game.debug_advance(0.2)
		var after := _rider.global_transform.basis.z
		var turn := 1.0 - clampf(before.dot(after), -1.0, 1.0)
		if turn > best_turn:
			best_turn = turn
			best = distance
		distance += 400.0
	print("WORLD_HAIRPIN distance=%.0f turn=%.2f" % [best, best_turn])
	return best


func _look_down() -> void:
	var at := _rider.global_transform
	_free.fov = 50.0
	_free.look_at_from_position(at.origin + Vector3(0, 240, 20), at.origin, Vector3(0, 0, -1))
	_free.current = true


func _arm_shot(path: String, band: bool) -> void:
	if path.begins_with(_rider_dir) and _rider_dir.is_empty():
		return
	_band = band
	_shot_path = path


func _save_shot() -> void:
	var image := _viewport.get_texture().get_image()
	if image == null or image.get_width() == 0:
		push_error("empty picture " + _shot_path)
		return
	if _band:
		_paint_band(image)
	image.save_png(_shot_path)
	print("WORLD_SHOT %s %dx%d" % [_shot_path, image.get_width(), image.get_height()])


func _paint_band(image: Image) -> void:
	var height := image.get_height()
	var width := image.get_width()
	var top := int(height * 0.75)
	for y in range(top, height):
		for x in range(width):
			image.set_pixel(x, y, image.get_pixel(x, y).lerp(Color(0.04, 0.05, 0.07), 0.5))
	for x in range(width):
		image.set_pixel(x, top, Color(0.95, 0.78, 0.25))


func _hide_ui() -> void:
	for layer in _game.find_children("*", "CanvasLayer", true, false):
		(layer as CanvasItem).visible = false


func _show_ui() -> void:
	for layer in _game.find_children("*", "CanvasLayer", true, false):
		(layer as CanvasItem).visible = true


func _count(label: String) -> void:
	var world := _game.get_node_or_null("World")
	if world == null:
		print("WORLD_STATS %s missing" % label)
		return
	var tally := {"nodes": 0, "tris": 0}
	_walk(world, tally)
	print("WORLD_STATS %s nodes=%d tris=%d" % [label, tally["nodes"], tally["tris"]])


func _walk(node: Node, tally: Dictionary) -> void:
	tally["nodes"] += 1
	if node is MeshInstance3D:
		var mesh := (node as MeshInstance3D).mesh
		if mesh != null:
			for surface in mesh.get_surface_count():
				var indexes: int = mesh.surface_get_array_index_len(surface)
				var verts: int = mesh.surface_get_array_len(surface)
				tally["tris"] += indexes / 3 if indexes > 0 else verts / 3
	if node is MultiMeshInstance3D:
		var multi := (node as MultiMeshInstance3D).multimesh
		if multi != null and multi.mesh != null:
			var one := 0
			for surface in multi.mesh.get_surface_count():
				var indexes: int = multi.mesh.surface_get_array_index_len(surface)
				var verts: int = multi.mesh.surface_get_array_len(surface)
				one += indexes / 3 if indexes > 0 else verts / 3
			tally["tris"] += one * multi.visible_instance_count
	for child in node.get_children():
		_walk(child, tally)


func _finish() -> void:
	print("WORLD_DONE")
	if RenderingServer.frame_post_draw.is_connected(_on_drawn):
		RenderingServer.frame_post_draw.disconnect(_on_drawn)
	_game.free()
	_viewport.free()
	quit(0)
