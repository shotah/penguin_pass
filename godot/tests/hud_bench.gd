extends SceneTree

## Frame time of the ride with vsync forced off.
##
## The stock project is vsync-locked. This script turns vsync off, renders
## through a SubViewport, and prints PENGUIN_PASS_FRAME_MS. Always pass
## --fake-trainer or --no-bluetooth, plus --settings and --dashboard-port 0.

var _frames := 160
var _warmup := 40
var _tag := "hud"
var _samples: Array[float] = []
var _seen := 0
var _last_usec := 0
var _viewport: SubViewport
var _game: Node


func _init() -> void:
	_read_args()
	DisplayServer.window_set_vsync_mode(DisplayServer.VSYNC_DISABLED)
	Engine.max_fps = 0
	_viewport = SubViewport.new()
	_viewport.name = "BenchViewport"
	_viewport.size = Vector2i(1280, 800)
	_viewport.own_world_3d = true
	_viewport.render_target_update_mode = SubViewport.UPDATE_ALWAYS
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
	print("PENGUIN_PASS_VSYNC %s" % DisplayServer.window_get_vsync_mode())
	RenderingServer.frame_post_draw.connect(_on_drawn)


func _read_args() -> void:
	var args := OS.get_cmdline_user_args()
	_frames = int(_arg(args, "--frames", str(_frames)))
	_tag = _arg(args, "--tag", _tag)


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
	if _seen == 2:
		_game.debug_seek(900.0, 250.0, 0.0)
		_game.debug_set_target_power(250.0)
	if _seen <= _warmup:
		return
	_samples.append(dt_ms)
	if _samples.size() < _frames:
		return
	_finish(dt_ms)


func _finish(last_ms: float) -> void:
	if RenderingServer.frame_post_draw.is_connected(_on_drawn):
		RenderingServer.frame_post_draw.disconnect(_on_drawn)
	_samples.sort()
	var total := 0.0
	for sample in _samples:
		total += sample
	var p95 := int(float(_samples.size() - 1) * 0.95)
	print(
		"PENGUIN_PASS_FRAME_MS tag=%s n=%d min=%.2f avg=%.2f p95=%.2f max=%.2f last=%.2f"
		% [
			_tag,
			_samples.size(),
			_samples[0],
			total / float(_samples.size()),
			_samples[p95],
			_samples[_samples.size() - 1],
			last_ms,
		]
	)
	if _game != null and is_instance_valid(_game):
		_game.free()
	if _viewport != null and is_instance_valid(_viewport):
		_viewport.free()
	quit(0)
