extends SceneTree

## Ride HUD pictures. Renders through a SubViewport at the requested size.
##
## Always launch with --fake-trainer or --no-bluetooth, --settings, and
## --dashboard-port 0. Pass --skip-title so the console is on screen.

var _dir := "/tmp/penguin-pass-hud"
var _viewport: SubViewport
var _game: Node
var _views: Array = []
var _view := 0
var _settle := 0


func _init() -> void:
	_read_args()
	_viewport = SubViewport.new()
	_viewport.name = "HudShot"
	_viewport.size = Vector2i(1280, 800)
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
	# Trainer shots are last: connecting sticks for the rest of the process.
	_views = [
		["start", 40.0, 150.0, false, 1280, 800],
		["start-1080", 40.0, 150.0, false, 1920, 1080],
		["climb", 900.0, 280.0, false, 1280, 800],
		["climb-1080", 900.0, 280.0, false, 1920, 1080],
		["descent", 3800.0, 0.0, false, 1280, 800],
		["descent-1080", 3800.0, 0.0, false, 1920, 1080],
		["sprint", 900.0, 1085.0, false, 1280, 800],
		["sprint-1080", 900.0, 1085.0, false, 1920, 1080],
		["missing", 200.0, 220.0, true, 1280, 800],
		["missing-1080", 200.0, 220.0, true, 1920, 1080],
		["trainer", 900.0, 250.0, false, 1280, 800],
		["trainer-1080", 900.0, 250.0, false, 1920, 1080],
	]
	RenderingServer.frame_post_draw.connect(_on_drawn)


func _read_args() -> void:
	var args := OS.get_cmdline_user_args()
	_dir = _arg(args, "--dir", _dir)


func _arg(args: PackedStringArray, name: String, fallback: String) -> String:
	var index := args.find(name)
	if index >= 0 and index + 1 < args.size():
		return args[index + 1]
	return fallback


func _on_drawn() -> void:
	if _settle == 0 and _view == 0 and _game != null:
		# Let `_ready` finish before the first pose.
		_settle = -1
		return
	if _settle < 0:
		_settle = 0
	if _settle == 0:
		if _view >= _views.size():
			if RenderingServer.frame_post_draw.is_connected(_on_drawn):
				RenderingServer.frame_post_draw.disconnect(_on_drawn)
			print("PENGUIN_PASS_HUD_DONE")
			if _game != null and is_instance_valid(_game):
				_game.free()
			if _viewport != null and is_instance_valid(_viewport):
				_viewport.free()
			quit(0)
			return
		var view: Array = _views[_view]
		_viewport.size = Vector2i(int(view[4]), int(view[5]))
		_game.process_mode = Node.PROCESS_MODE_INHERIT
		_game.debug_menu_close()
		# Seek under power so a coasting pose can still reach the distance.
		var shown_power := float(view[2])
		var seek_power := shown_power if shown_power > 1.0 else 220.0
		_game.debug_seek(float(view[1]), seek_power, 0.0)
		if shown_power <= 1.0:
			_game.debug_set_target_power(0.0)
			_game.debug_advance(0.4)
		if str(view[0]).begins_with("trainer"):
			_game.debug_menu_open()
			_game.debug_menu_action("accept")
			_game.debug_advance(0.35)
			_game.debug_menu_action("accept")
			_game.debug_advance(0.6)
			_game.debug_menu_close()
		if bool(view[3]):
			_game.process_mode = Node.PROCESS_MODE_DISABLED
			_game.debug_show_missing_cadence()
		_settle = 1
		return
	_settle += 1
	if _settle < 8:
		return
	var view: Array = _views[_view]
	if bool(view[3]):
		_game.debug_show_missing_cadence()
	var path := _dir.path_join(str(view[0]) + ".png")
	if _save(path) != OK:
		quit(1)
		return
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
