extends SceneTree

## Overhead, valley, and eight-way saddle sheets. HUD hidden.

var _viewport: SubViewport
var _game: Node
var _rider: Node3D
var _free: Camera3D
var _seen := 0
var _queue: Array = []
var _shot := ""
var _out := ""
var _sheet: Array = []
var _sheet_name := ""


func _init() -> void:
	var args := OS.get_cmdline_user_args()
	_out = _arg(args, "--out-dir", "")
	DirAccess.make_dir_recursive_absolute(_out)
	_viewport = SubViewport.new()
	_viewport.size = Vector2i(1280, 800)
	_viewport.own_world_3d = true
	_viewport.render_target_update_mode = SubViewport.UPDATE_ALWAYS
	root.add_child(_viewport)
	_game = (load("res://main.tscn") as PackedScene).instantiate()
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
	if _shot != "":
		var image := _viewport.get_texture().get_image()
		if image != null:
			image.save_png(_shot)
			print("SHEET_FRAME %s" % _shot)
			if _sheet_name != "":
				_sheet.append(image.duplicate())
		_shot = ""
		return
	if _queue.is_empty():
		if _rider == null:
			_setup()
		else:
			_flush_sheet()
			print("SHEET_DONE")
			_game.free()
			quit(0)
		return
	var job: Callable = _queue.pop_front()
	job.call()


func _setup() -> void:
	if _game.has_method("debug_start_ride"):
		_game.debug_start_ride()
	_rider = _game.get_node("Rider")
	_free = Camera3D.new()
	_free.near = 0.08
	_free.far = 20000.0
	_viewport.add_child(_free)
	_hide_hud()
	_overhead()
	_valley()
	_ring("start", 12.0)
	_ring("climb", 900.0)
	_ring("summit", 2100.0)
	_ring("descent", 3800.0)
	_fp(1280, 800, "fp-1280")
	_queue.append(func() -> void:
		DisplayServer.window_set_size(Vector2i(1920, 1080))
		_viewport.size = Vector2i(1920, 1080)
	)
	_fp(1920, 1080, "fp-1080")


func _hide_hud() -> void:
	for node in _game.find_children("*", "CanvasLayer", true, false):
		node.visible = false


func _overhead() -> void:
	_queue.append(func() -> void:
		_game.debug_set_first_person(false)
		_game.debug_seek(1500.0, 160.0, 0.0)
		_game.debug_advance(0.2)
		var at := _rider.global_position
		_free.fov = 60.0
		_free.look_at_from_position(at + Vector3(0, 1500, 1), at, Vector3(0, 0, -1))
		_free.current = true
		_shot = _out.path_join("overhead-1500.png")
	)


func _valley() -> void:
	_queue.append(func() -> void:
		_game.debug_set_first_person(false)
		_game.debug_seek(40.0, 160.0, 0.0)
		_game.debug_advance(0.3)
		var basis := _rider.global_transform.basis
		var forward := (-basis.z).normalized()
		var at := _rider.global_position
		_free.fov = 55.0
		_free.look_at_from_position(at + Vector3(0, 250, 0) - forward * 30.0, at + forward * 600.0, Vector3.UP)
		_free.current = true
		_shot = _out.path_join("valley-250.png")
	)


func _ring(place: String, distance: float) -> void:
	_queue.append(func() -> void:
		_flush_sheet()
		_sheet_name = place
		_game.debug_set_first_person(false)
		_game.debug_seek(distance, 180.0, 0.0)
		_game.debug_advance(0.4)
	)
	for step in 8:
		_queue.append(_aim.bind(step))


func _aim(step: int) -> void:
	var basis := _rider.global_transform.basis
	var forward := (-basis.z).normalized()
	var dir := forward.rotated(Vector3.UP, step * TAU / 8.0)
	var eye := _rider.global_position + Vector3(0, 1.55, 0)
	_free.fov = 60.0
	_free.look_at_from_position(eye, eye + dir * 120.0, Vector3.UP)
	_free.current = true
	_shot = _out.path_join("%s-%d.png" % [_sheet_name, step])


func _fp(width: int, _height: int, name: String) -> void:
	_queue.append(func() -> void:
		_game.debug_set_first_person(true)
		_game.debug_seek(20.0, 160.0, 0.0)
		_game.debug_advance(0.4)
		var camera := _game.get_node("CameraRig/Camera") as Camera3D
		camera.current = true
		_shot = _out.path_join("%s.png" % name)
	)


func _flush_sheet() -> void:
	if _sheet.is_empty() or _sheet_name == "":
		_sheet.clear()
		return
	var sheet := Image.create(1280 * 4, 800 * 2, false, Image.FORMAT_RGB8)
	for index in _sheet.size():
		var frame: Image = _sheet[index]
		frame.convert(Image.FORMAT_RGB8)
		var column := index % 4
		var row := index / 4
		sheet.blit_rect(frame, Rect2i(0, 0, 1280, 800), Vector2i(column * 1280, row * 800))
	var path := _out.path_join("sheet-%s.png" % _sheet_name)
	sheet.save_png(path)
	print("SHEET %s" % path)
	_sheet.clear()
