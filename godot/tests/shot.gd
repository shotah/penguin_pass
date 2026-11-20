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
var _frames := 24
var _seen := 0
var _warmup := 20
var _samples: Array[float] = []
var _last_usec := 0
var _viewport: SubViewport
var _game: Node


func _init() -> void:
	_read_args()
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
	if _seen == 2:
		_game.debug_seek(_distance, _power, _elapsed)
		_game.debug_set_first_person(_first_person)
	if _seen <= _warmup:
		return
	_samples.append(dt_ms)
	if _samples.size() < _frames:
		return
	_finish(dt_ms)


func _finish(last_ms: float) -> void:
	if RenderingServer.frame_post_draw.is_connected(_on_drawn):
		RenderingServer.frame_post_draw.disconnect(_on_drawn)
	var image := _viewport.get_texture().get_image()
	if image == null or image.get_width() == 0:
		push_error("viewport image was empty")
		quit(1)
		return
	var dir := _out.get_base_dir()
	if not dir.is_empty():
		DirAccess.make_dir_recursive_absolute(dir)
	# SubViewport images are already upright on this Godot. Do not flip them.
	var err := image.save_png(_out)
	if err != OK:
		push_error("save_png failed: %s" % err)
		quit(1)
		return
	print("PENGUIN_PASS_SHOT %s %dx%d" % [_out, image.get_width(), image.get_height()])
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
	quit(0)
