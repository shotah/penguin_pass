extends SceneTree

## Rider views of the forest, plus a steady frame sample with no screenshot in it.

var _viewport: SubViewport
var _game: Node
var _free: Camera3D
var _seen := 0
var _queue: Array = []
var _shot := ""
var _out := ""
var _bench_left := 0
var _bench_warm := 0
var _bench_sum := 0.0
var _bench_n := 0
var _bench_p: Array = []
var _bench_draws := 0.0
var _bench_name := ""
var _last_usec := 0


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
	if _bench_left > 0:
		var now := Time.get_ticks_usec()
		if _last_usec > 0 and _bench_warm <= 0:
			var ms := float(now - _last_usec) / 1000.0
			_bench_sum += ms
			_bench_n += 1
			_bench_p.append(ms)
			_bench_draws += Performance.get_monitor(Performance.RENDER_TOTAL_DRAW_CALLS_IN_FRAME)
		elif _bench_warm > 0:
			_bench_warm -= 1
		_last_usec = now
		_bench_left -= 1
		if _bench_left == 0:
			_bench_p.sort()
			var avg: float = _bench_sum / float(maxi(_bench_n, 1))
			var p95: float = float(_bench_p[int(float(_bench_n) * 0.95)])
			var draws: float = _bench_draws / float(maxi(_bench_n, 1))
			print("TREE_BENCH %s avg_ms=%.3f p95_ms=%.3f draws=%.1f n=%d" % [_bench_name, avg, p95, draws, _bench_n])
			_bench_sum = 0.0
			_bench_n = 0
			_bench_draws = 0.0
			_bench_p = []
			_last_usec = 0
		return
	if _seen < 4:
		return
	if _shot != "":
		var image := _viewport.get_texture().get_image()
		if image != null:
			image.save_png(_shot)
			print("TREE_FRAME %s" % _shot)
		_shot = ""
		return
	if _queue.is_empty():
		if _free == null:
			_setup()
		else:
			print("TREE_DONE")
			quit(0)
		return
	var job: Callable = _queue.pop_front()
	job.call()


func _setup() -> void:
	if _game.has_method("debug_start_ride"):
		_game.debug_start_ride()
	_free = Camera3D.new()
	_free.near = 0.05
	_free.far = 20000.0
	_free.fov = 68.0
	_viewport.add_child(_free)
	var args := OS.get_cmdline_user_args()
	if args.has("--bench"):
		for item in [["start", 80.0], ["climb", 900.0], ["switchback", 2400.0]]:
			_queue.append(_begin_bench.bind(str(item[0]), float(item[1])))
		return
	_queue.append(_shoot.bind("start", 80.0, 28.0))
	_queue.append(_shoot.bind("climb", 900.0, -32.0))
	_queue.append(_shoot.bind("switchback", 2400.0, 24.0))


func _place(distance: float, yaw_deg: float) -> void:
	_free.current = false
	_game.debug_set_first_person(true)
	_game.debug_seek(distance, 180.0, 0.0)
	_game.debug_advance(0.05)
	var cam := _game.find_child("Camera", true, false) as Camera3D
	if cam != null:
		_free.global_transform = cam.global_transform
		_free.fov = cam.fov
		_free.rotate_object_local(Vector3.UP, deg_to_rad(yaw_deg))
		_free.rotate_object_local(Vector3.RIGHT, deg_to_rad(-6.0))
	_free.current = true


func _shoot(name: String, distance: float, yaw_deg: float) -> void:
	_place(distance, yaw_deg)
	_shot = _out.path_join("%s.png" % name)


func _begin_bench(name: String, distance: float) -> void:
	_place(distance, 0.0)
	_bench_name = name
	_bench_warm = 40
	_bench_left = 120
	_bench_sum = 0.0
	_bench_n = 0
	_bench_draws = 0.0
	_bench_p = []
	_last_usec = 0
