extends SceneTree

## Rider's-eye frames of each landmark, plus the horizon peak.

var _viewport: SubViewport
var _game: Node
var _rider: Node3D
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
	if _seen < 4:
		return
	if _shot != "":
		var image := _viewport.get_texture().get_image()
		if image != null:
			image.save_png(_shot)
			var ms := Performance.get_monitor(Performance.TIME_PROCESS) * 1000.0
			print("LANDMARK_FRAME %s %.3f" % [_shot, ms])
		_shot = ""
		return
	if _queue.is_empty():
		if _rider == null:
			_setup()
		else:
			print("LANDMARK_DONE")
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
	_free.near = 0.05
	_free.far = 20000.0
	_free.fov = 68.0
	_viewport.add_child(_free)
	var args := OS.get_cmdline_user_args()
	var prefix := _arg(args, "--prefix", "alpine")
	if not args.has("--horizon-only"):
		for node in _game.find_children("Landmark_*", "Node3D", true, false):
			_queue_marker(prefix, node)
	var peak := _game.find_child("HorizonPeak", true, false) as Node3D
	if peak != null:
		for place in [["start", 20.0], ["climb", 900.0], ["summit", 2100.0]]:
			_queue.append(_peak.bind(prefix, peak, str(place[0]), float(place[1])))


func _queue_marker(prefix: String, node: Node3D) -> void:
	var id := str(node.name).trim_prefix("Landmark_")
	var reveal := float(node.get_meta("reveal_m", 0.0))
	_queue.append(_rider_eye.bind(prefix, id, "before", max(reveal - 70.0, 0.0)))
	_queue.append(_rider_eye.bind(prefix, id, "reveal", reveal))
	_queue.append(_beside.bind(prefix, node, id))


func _rider_eye(prefix: String, id: String, label: String, distance: float) -> void:
	_free.current = false
	_game.debug_set_first_person(true)
	_game.debug_seek(distance, 180.0, 0.0)
	_game.debug_advance(0.05)
	var cam := _game.find_child("Camera", true, false) as Camera3D
	if cam != null:
		_free.global_transform = cam.global_transform
		_free.fov = cam.fov
	_free.current = true
	_shot = _out.path_join("%s-%s-%s.png" % [prefix, id, label])


func _beside(prefix: String, node: Node3D, id: String) -> void:
	var basis := node.global_transform.basis
	var eye := node.global_position - basis.z * 22.0 + basis.x * 16.0 + Vector3(0, 5.0, 0)
	_free.look_at_from_position(eye, node.global_position + Vector3(0, 6.0, 0), Vector3.UP)
	_free.fov = 50.0
	_free.current = true
	_shot = _out.path_join("%s-%s-beside.png" % [prefix, id])


func _peak(prefix: String, _peak: Node3D, label: String, distance: float) -> void:
	_rider_eye(prefix, "horizon", label, distance)
