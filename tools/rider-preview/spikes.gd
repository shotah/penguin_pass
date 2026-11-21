extends SceneTree

## Layer toggles so each kind of spike can be named. Not a kept picture set.

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
		image.save_png(_shot)
		print("SPIKE_SHOT %s" % _shot)
		_shot = ""
		return
	if _queue.is_empty():
		if _rider == null:
			_setup()
		else:
			print("SPIKE_DONE")
			_game.free()
			_viewport.free()
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
	_free.far = 8000.0
	_viewport.add_child(_free)
	var hud := _game.get_node_or_null("Hud")
	if hud != null:
		hud.visible = false
	_enqueue("builtin", 900.0, "all")
	_enqueue("builtin", 2500.0, "all")
	_enqueue("builtin", 3800.0, "all")
	_enqueue("builtin", 3800.0, "noback")
	_enqueue("builtin", 3800.0, "nofill")
	_enqueue("builtin", 3800.0, "noscatter")
	_enqueue("builtin", 3800.0, "norail")
	_queue.append(func() -> void:
		_game.debug_load_course("mountain", 10, 11)
		print("SPIKE_LOADED mountain")
	)
	_enqueue("mountain", 2000.0, "all")
	_enqueue("mountain", 4500.0, "all")
	_enqueue("mountain", 2000.0, "noback")
	_enqueue("mountain", 2000.0, "nofill")
	_enqueue("mountain", 2000.0, "noscatter")


func _enqueue(course: String, distance: float, mode: String) -> void:
	_queue.append(func() -> void:
		_apply(mode)
		_game.debug_set_first_person(true)
		_game.debug_seek(distance, 200.0, 0.0)
		_game.debug_advance(1.0)
		_camera.current = true
		_shot = _out.path_join("%s-%.0f-%s-saddle.png" % [course, distance, mode])
	)
	_queue.append(func() -> void:
		var at := _rider.global_transform
		_free.fov = 55.0
		_free.look_at_from_position(at.origin + Vector3(0, 180, 40), at.origin, Vector3(0, 0, -1))
		_free.current = true
		_shot = _out.path_join("%s-%.0f-%s-above.png" % [course, distance, mode])
	)


func _apply(mode: String) -> void:
	var world := _game.get_node("World")
	_show_all(world)
	if mode == "noback":
		_hide_named(world, "Backdrop")
	elif mode == "nofill":
		_hide_named(world, "Fill")
	elif mode == "norail":
		_hide_named(world, "GuardRails")
	elif mode == "noscatter":
		for child in world.get_children():
			var name := str(child.name)
			if name.begins_with("tree") or name.begins_with("bush") or name.begins_with("grass") or name.begins_with("flower") or name.begins_with("rock"):
				(child as Node3D).visible = false


func _show_all(world: Node) -> void:
	for child in world.get_children():
		if child is Node3D:
			(child as Node3D).visible = true
	var terrain := world.get_node_or_null("Terrain")
	if terrain != null:
		for child in terrain.get_children():
			if child is Node3D:
				(child as Node3D).visible = true


func _hide_named(world: Node, name: String) -> void:
	var node := world.find_child(name, true, false)
	if node is Node3D:
		(node as Node3D).visible = false
