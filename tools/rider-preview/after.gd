extends SceneTree

var _viewport: SubViewport
var _game: Node
var _rider: Node3D
var _camera: Camera3D
var _free: Camera3D
var _seen := 0
var _queue: Array = []
var _shot := ""
var _out := ""


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
			print("AFTER_SHOT %s" % _shot)
		_shot = ""
		return
	if _queue.is_empty():
		if _rider == null:
			_setup()
		else:
			print("AFTER_DONE")
			_game.free()
			quit(0)
		return
	var job: Callable = _queue.pop_front()
	job.call()


func _setup() -> void:
	if _game.has_method("debug_start_ride"):
		_game.debug_start_ride()
	_rider = _game.get_node("Rider")
	_camera = _game.get_node("CameraRig/Camera")
	_free = Camera3D.new()
	_free.near = 0.08
	_free.far = 20000.0
	_viewport.add_child(_free)
	var hud := _game.get_node_or_null("Hud")
	if hud != null:
		hud.visible = false
	# The owner's two viewpoints, then the rest of the loop.
	_view("alpine-climb", 900.0, false)
	_view("alpine-start", 8.0, true)
	_view("alpine-descent", 3800.0, false)
	_view("alpine-summit", 2100.0, false)
	_view("alpine-bend", 340.0, false)
	_side("alpine-climb-side", 900.0)
	_side("alpine-descent-side", 3800.0)
	_high("alpine-high")
	_queue.append(func() -> void:
		var t := Time.get_ticks_msec()
		var ok: bool = _game.debug_load_course("mountain", 10, 11)
		print("AFTER_LOAD mountain ok=%s ms=%d" % [ok, Time.get_ticks_msec() - t])
	)
	_view("mountain-hairpin", 2000.0, false)
	_view("mountain-hairpin", 2000.0, true)
	_view("mountain-road", 5200.0, false)
	_view("mountain-road", 5200.0, true)
	_above("mountain-above", 2000.0)
	_queue.append(func() -> void:
		var t := Time.get_ticks_msec()
		var ok: bool = _game.debug_load_course("desert", 10, 7)
		print("AFTER_LOAD desert ok=%s ms=%d" % [ok, Time.get_ticks_msec() - t])
	)
	_view("desert-road", 700.0, false)
	_view("desert-road", 700.0, true)
	_view("desert-far", 3500.0, false)
	_side("desert-side", 1400.0)
	_above("desert-above", 700.0)


func _view(name: String, distance: float, first: bool) -> void:
	_queue.append(func() -> void:
		_game.debug_set_first_person(first)
		_game.debug_seek(distance, 200.0, 0.0)
		_game.debug_advance(1.2)
		_camera.current = true
		var eye := ""
		if first and _rider.has_method("eye_position"):
			eye = " eye=%.2f" % _camera.global_position.distance_to(_rider.eye_position())
		print("AFTER_AT %s speed=%.1f%s" % [name, float(_game.speed_kph()), eye])
		_shot = _out.path_join("%s-%s.png" % [name, "fp" if first else "chase"])
	)


func _high(name: String) -> void:
	_queue.append(func() -> void:
		_game.debug_set_first_person(false)
		_game.debug_seek(1500.0, 180.0, 0.0)
		_game.debug_advance(0.2)
		var at := _rider.global_transform
		_free.fov = 55.0
		_free.look_at_from_position(at.origin + Vector3(0, 2800, 400), at.origin, Vector3(0, 0, -1))
		_free.current = true
		_shot = _out.path_join("%s.png" % name)
	)


func _above(name: String, distance: float) -> void:
	_queue.append(func() -> void:
		_game.debug_set_first_person(false)
		_game.debug_seek(distance, 200.0, 0.0)
		_game.debug_advance(0.3)
		var at := _rider.global_transform
		_free.fov = 50.0
		_free.look_at_from_position(at.origin + Vector3(0, 160, 30), at.origin, Vector3(0, 0, -1))
		_free.current = true
		_shot = _out.path_join("%s.png" % name)
	)


func _side(name: String, distance: float) -> void:
	_queue.append(func() -> void:
		_game.debug_set_first_person(false)
		_game.debug_seek(distance, 200.0, 0.0)
		_game.debug_advance(0.8)
		var at := _rider.global_transform
		var side := at.basis.x.normalized()
		_free.fov = 55.0
		_free.look_at_from_position(at.origin + Vector3(0, 1.7, 0), at.origin + side * 180.0 + Vector3(0, 4, 0), Vector3.UP)
		_free.current = true
		_shot = _out.path_join("%s.png" % name)
	)
