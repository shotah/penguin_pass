extends SceneTree

## Headless check: the extension loads, the ride moves, and missing sensors
## render as an em dash. A failed check quits non-zero and skips the sentinel.

func _init() -> void:
	var version := _probe()
	if version.is_empty():
		return
	var packed := load("res://main.tscn") as PackedScene
	if packed == null:
		_fail("main.tscn failed to load")
		return
	var game := packed.instantiate()
	if game == null:
		_fail("main scene failed to instantiate")
		return
	# `_ready` is deferred when the node is added from SceneTree._init.
	# Exercise the ride on the first process frame, after the scene has built.
	root.add_child(game)
	_game = game
	_version = version
	process_frame.connect(_on_frame)


var _game: Node
var _version := ""
var _ran := false


func _on_frame() -> void:
	if _ran:
		return
	_ran = true
	_exercise(_game, _version)


func _probe() -> String:
	var penguin_class := &"PenguinPass"
	if not ClassDB.class_exists(penguin_class):
		_fail("PenguinPass is not registered")
		return ""
	if not ClassDB.class_exists(&"Game"):
		_fail("Game is not registered")
		return ""
	var instance = ClassDB.instantiate(penguin_class)
	if instance == null:
		_fail("PenguinPass could not be instantiated")
		return ""
	var version := str(instance.version())
	if instance is Node:
		instance.free()
	return version


func _exercise(game: Node, version: String) -> void:
	game.debug_set_target_power(200.0)
	for _i in 180:
		game.debug_advance(1.0 / 60.0)
	var distance: float = game.distance_m()
	if distance <= 5.0:
		_fail("distance did not increase: %s" % distance)
		return
	if str(game.hud_power_text()) != "200":
		_fail("power label '%s'" % game.hud_power_text())
		return
	if str(game.hud_cadence_text()) != "85":
		_fail("cadence label '%s'" % game.hud_cadence_text())
		return
	if str(game.hud_heart_text()) != "—":
		_fail("heart rate should be an em dash, got '%s'" % game.hud_heart_text())
		return
	if float(game.speed_kph()) <= 5.0:
		_fail("speed did not rise: %s" % game.speed_kph())
		return
	var speed_text := str(game.hud_speed_text())
	if speed_text.find(".") < 0:
		_fail("speed should show one decimal, got '%s'" % speed_text)
		return
	var grade_text := str(game.hud_grade_text())
	if grade_text.find(".") < 0:
		_fail("grade should show one decimal, got '%s'" % grade_text)
		return
	var elevation := str(game.hud_elevation_text())
	if elevation == "0" or elevation == "—":
		_fail("elevation should be a real altitude, got '%s'" % elevation)
		return
	if int(game.profile_point_count()) < 2:
		_fail("elevation profile is empty")
		return
	if int(game.lap()) < 1:
		_fail("lap should start at 1")
		return
	if str(game.hud_status_text()).find("Simulated") < 0:
		_fail("status line missing, got '%s'" % game.hud_status_text())
		return
	game.debug_set_target_power(9999.0)
	if abs(float(game.power_w()) - 1500.0) > 0.01:
		_fail("power did not clamp to 1500: %s" % game.power_w())
		return
	game.debug_set_target_power(-50.0)
	if abs(float(game.power_w())) > 0.01:
		_fail("power did not clamp to 0: %s" % game.power_w())
		return
	game.debug_advance(1.0 / 60.0)
	if str(game.hud_cadence_text()) != "0":
		_fail("coasting cadence should be 0, got '%s'" % game.hud_cadence_text())
		return
	game.debug_show_missing_cadence()
	if str(game.hud_cadence_text()) != "—":
		_fail("missing cadence should be an em dash, got '%s'" % game.hud_cadence_text())
		return
	if str(game.hud_heart_text()) != "—":
		_fail("missing heart rate should be an em dash, got '%s'" % game.hud_heart_text())
		return
	if str(game.hud_power_text()) == "—":
		_fail("power is not a missing sensor")
		return
	print("PENGUIN_PASS_SMOKE_OK %s" % version)
	quit(0)


func _fail(message: String) -> void:
	push_error(message)
	print("PENGUIN_PASS_SMOKE_FAIL %s" % message)
	quit(1)
