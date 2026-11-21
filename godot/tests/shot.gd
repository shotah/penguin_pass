extends SceneTree

## Grab one frame of the ride.
##
## User args (after --):
##   --out PATH --width N --height N --distance M --power W --elapsed S
##   --camera chase|first --bench --frames N
##
## Renders through a SubViewport so the shot is the requested size even when
## the monitor is smaller. Quit is unconditional.

var _out := "/tmp/penguin-pass-shots/shot.png"
var _width := 1280
var _height := 800
var _distance := 1200.0
var _power := 220.0
var _elapsed := 754.0
var _first_person := false
var _bench := false
var _set := false
var _dir := ""
var _view := 0
var _settle := 0
var _view_samples: Array[float] = []
var _frames := 24
var _seen := 0
var _warmup := 20
var _samples: Array[float] = []
var _last_usec := 0
var _viewport: SubViewport
var _game: Node


func _init() -> void:
	_read_args()
	var user_args := OS.get_cmdline_user_args()
	if not user_args.has("--fake-trainer") and not user_args.has("--no-bluetooth"):
		OS.set_environment("PENGUIN_PASS_FAKE_TRAINER", "1")
		OS.set_environment("PENGUIN_PASS_DASHBOARD_PORT", "0")
		OS.set_environment("PENGUIN_PASS_DASHBOARD_LOOPBACK", "1")
		if not user_args.has("--settings"):
			var settings_path := "/tmp/penguin-pass-shot-%d.toml" % OS.get_process_id()
			var settings_file := FileAccess.open(settings_path, FileAccess.WRITE)
			if settings_file != null:
				settings_file.store_string("fullscreen = false\ntv_dashboard_enabled = true\n")
				settings_file.close()
				OS.set_environment("PENGUIN_PASS_SETTINGS", settings_path)
	_viewport = SubViewport.new()
	_viewport.name = "ShotViewport"
	_viewport.size = Vector2i(_width, _height)
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
	_last_usec = Time.get_ticks_usec()
	RenderingServer.frame_post_draw.connect(_on_drawn)


func _release() -> void:
	if _game != null and is_instance_valid(_game):
		_game.free()
		_game = null
	if _viewport != null and is_instance_valid(_viewport):
		_viewport.free()
		_viewport = null


func _read_args() -> void:
	var args := OS.get_cmdline_user_args()
	_out = _arg(args, "--out", _out)
	_width = int(_arg(args, "--width", str(_width)))
	_height = int(_arg(args, "--height", str(_height)))
	_distance = float(_arg(args, "--distance", str(_distance)))
	_power = float(_arg(args, "--power", str(_power)))
	_elapsed = float(_arg(args, "--elapsed", str(_elapsed)))
	_frames = int(_arg(args, "--frames", str(_frames)))
	_first_person = _arg(args, "--camera", "chase") == "first"
	_bench = args.has("--bench")
	_set = args.has("--set")
	_dir = _arg(args, "--dir", "")
	if _bench:
		_frames = max(_frames, 240)
		_warmup = 60


func _arg(args: PackedStringArray, name: String, fallback: String) -> String:
	var index := args.find(name)
	if index >= 0 and index + 1 < args.size():
		return args[index + 1]
	return fallback


func _on_drawn() -> void:
	var now := Time.get_ticks_usec()
	var dt_ms := float(now - _last_usec) / 1000.0
	_last_usec = now
	_seen += 1
	# SceneTree._init adds the scene before `_ready`. Pose once ready has run.
	if _set:
		_on_set_frame(dt_ms)
		return
	if _seen == 2:
		_game.debug_seek(_distance, _power, _elapsed)
		_game.debug_set_first_person(_first_person)
	if _seen <= _warmup:
		return
	_samples.append(dt_ms)
	if _samples.size() < _frames:
		return
	_finish(dt_ms)


func _views() -> Array:
	return [
		["start-chase", 8.0, false, 1280, 800, "ride"],
		["start-fp", 8.0, true, 1280, 800, "ride"],
		["switchback-chase", 340.0, false, 1280, 800, "ride"],
		["switchback-fp", 340.0, true, 1280, 800, "ride"],
		["climb-chase", 900.0, false, 1280, 800, "ride"],
		["climb-fp", 900.0, true, 1280, 800, "ride"],
		["summit-chase", 2500.0, false, 1280, 800, "ride"],
		["summit-fp", 2500.0, true, 1280, 800, "ride"],
		["descent-chase", 3800.0, false, 1280, 800, "ride"],
		["descent-fp", 3800.0, true, 1280, 800, "ride"],
		["tv-1080", 900.0, false, 1920, 1080, "ride"],
		["menu-main", 8.0, false, 1280, 800, "menu"],
		["menu-pairing", 8.0, false, 1280, 800, "pairing"],
		["menu-connected", 8.0, false, 1280, 800, "connected"],
		["hud-trainer", 900.0, false, 1280, 800, "hud"],
	]


func _pose(kind: String) -> void:
	_game.debug_menu_close()
	if kind == "menu":
		_game.debug_menu_open()
	elif kind == "pairing":
		_game.debug_menu_open()
		_game.debug_menu_action("accept")
		_game.debug_advance(0.35)
	elif kind == "connected" or kind == "hud":
		_game.debug_menu_open()
		_game.debug_menu_action("accept")
		_game.debug_advance(0.35)
		_game.debug_menu_action("accept")
		_game.debug_advance(0.6)
		if kind == "hud":
			_game.debug_menu_close()


func _on_set_frame(dt_ms: float) -> void:
	if _seen < 3:
		return
	if _settle == 0:
		if _view >= _views().size():
			if RenderingServer.frame_post_draw.is_connected(_on_drawn):
				RenderingServer.frame_post_draw.disconnect(_on_drawn)
			print("PENGUIN_PASS_SET_DONE")
			_release()
			quit(0)
			return
		var view: Array = _views()[_view]
		_viewport.size = Vector2i(int(view[3]), int(view[4]))
		_game.debug_seek(float(view[1]), _power, 0.0)
		_game.debug_set_first_person(bool(view[2]))
		_pose(str(view[5]) if view.size() > 5 else "ride")
		_view_samples.clear()
		_settle = 1
		return
	_settle += 1
	if _settle > 3:
		_view_samples.append(dt_ms)
	if _settle < 12:
		return
	var view: Array = _views()[_view]
	var path := _dir.path_join(str(view[0]) + ".png")
	if _save(path) != OK:
		quit(1)
		return
	if _view_samples.size() > 0:
		var copy := _view_samples.duplicate()
		copy.sort()
		var total := 0.0
		for sample in copy:
			total += sample
		var p95 := int(float(copy.size() - 1) * 0.95)
		print(
			"PENGUIN_PASS_VIEW %s n=%d avg=%.2f p95=%.2f max=%.2f"
			% [view[0], copy.size(), total / float(copy.size()), copy[p95], copy[copy.size() - 1]]
		)
	_view += 1
	_settle = 0


func _save(path: String) -> int:
	var image := _viewport.get_texture().get_image()
	if image == null or image.get_width() == 0:
		push_error("viewport image was empty")
		return FAILED
	var dir := path.get_base_dir()
	if not dir.is_empty():
		DirAccess.make_dir_recursive_absolute(dir)
	var err := image.save_png(path)
	if err != OK:
		push_error("save_png failed: %s" % err)
		return err
	print("PENGUIN_PASS_SHOT %s %dx%d" % [path, image.get_width(), image.get_height()])
	return OK


func _finish(last_ms: float) -> void:
	if RenderingServer.frame_post_draw.is_connected(_on_drawn):
		RenderingServer.frame_post_draw.disconnect(_on_drawn)
	if _save(_out) != OK:
		quit(1)
		return
	if _bench and _samples.size() > 0:
		_samples.sort()
		var total := 0.0
		for sample in _samples:
			total += sample
		var avg := total / float(_samples.size())
		var p95_index := int(float(_samples.size() - 1) * 0.95)
		print(
			"PENGUIN_PASS_FRAME_MS n=%d min=%.2f avg=%.2f p95=%.2f max=%.2f last=%.2f"
			% [
				_samples.size(),
				_samples[0],
				avg,
				_samples[p95_index],
				_samples[_samples.size() - 1],
				last_ms,
			]
		)
	_release()
	quit(0)
