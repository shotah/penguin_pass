extends SceneTree

## Pictures and a steady frame average for time of day and weather.
##
## The frame average counts process frames after a warmup and does not call
## get_image, so the screenshot readback is not in the number.

var _viewport: SubViewport
var _game: Node
var _free: Camera3D
var _seen := 0
var _queue: Array = []
var _shot := ""
var _out := ""
var _bench := false
var _bench_name := ""
var _bench_left := 0
var _bench_warm := 0
var _bench_sum := 0.0
var _bench_n := 0
var _last_usec := 0


func _init() -> void:
	var args := OS.get_cmdline_user_args()
	_out = _arg(args, "--out-dir", "")
	_bench = args.has("--bench")
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
			_bench_sum += float(now - _last_usec) / 1000.0
			_bench_n += 1
		elif _bench_warm > 0:
			_bench_warm -= 1
		_last_usec = now
		_bench_left -= 1
		if _bench_left == 0:
			var avg: float = _bench_sum / float(maxi(_bench_n, 1))
			print("ATMOS_BENCH %s avg_ms=%.3f n=%d" % [_bench_name, avg, _bench_n])
			_bench_sum = 0.0
			_bench_n = 0
			_last_usec = 0
		return
	if _seen < 4:
		return
	if _shot != "":
		var image := _viewport.get_texture().get_image()
		if image != null:
			image.save_png(_shot)
			print("ATMOS_FRAME %s" % _shot)
		_shot = ""
		return
	if _queue.is_empty():
		if _free == null:
			_setup()
		else:
			print("ATMOS_DONE")
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
	var theme := "desert" if OS.get_environment("PENGUIN_PASS_THEME") == "desert" else "alpine"
	if _bench:
		for item in [["day", 12.0, "clear"], ["rain", 12.0, "rain"], ["snow", 12.0, "snow"], ["night", 22.5, "clear"]]:
			_queue.append(_begin_bench.bind(theme, str(item[0]), float(item[1]), str(item[2])))
		return
	if theme == "desert":
		_queue.append(_shoot.bind(theme, "desert-sunset", 17.2, "clear"))
	else:
		_queue.append(_shoot.bind(theme, "alpine-dawn", 6.7, "clear"))
		_queue.append(_shoot.bind(theme, "alpine-sunset", 17.2, "clear"))
		_queue.append(_shoot.bind(theme, "alpine-night", 22.5, "clear"))
		_queue.append(_shoot.bind(theme, "alpine-rain", 12.0, "rain"))
		_queue.append(_shoot.bind(theme, "alpine-snow", 12.0, "snow"))


func _place_camera() -> void:
	_free.current = false
	_game.debug_set_first_person(true)
	_game.debug_seek(240.0, 180.0, 0.0)
	_game.debug_advance(0.05)
	var cam := _game.find_child("Camera", true, false) as Camera3D
	if cam != null:
		_free.global_transform = cam.global_transform
		_free.fov = cam.fov
	_free.current = true


func _apply(theme: String, hour: float, weather: String) -> void:
	_place_camera()
	var atmo := _game.find_child("Atmosphere", true, false)
	var cam := _game.find_child("Camera", true, false) as Camera3D
	if atmo == null or cam == null:
		print("ATMOS_MISSING")
		return
	var forward := -cam.global_transform.basis.z
	atmo.apply_named(theme, hour, weather, cam.global_position.x, cam.global_position.y, cam.global_position.z, forward.x, forward.y, forward.z)
	_free.global_transform = cam.global_transform


func _shoot(theme: String, name: String, hour: float, weather: String) -> void:
	_apply(theme, hour, weather)
	_shot = _out.path_join("%s.png" % name)


func _begin_bench(theme: String, name: String, hour: float, weather: String) -> void:
	_apply(theme, hour, weather)
	_bench_name = name
	_bench_warm = 40
	_bench_left = 120
	_bench_sum = 0.0
	_bench_n = 0
	_last_usec = 0
