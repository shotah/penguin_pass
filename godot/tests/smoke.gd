extends SceneTree

## Headless check: the extension loads, the ride moves, and missing sensors
## render as an em dash. A failed check quits non-zero and skips the sentinel.

func _init() -> void:
	# ./x smoke cannot forward arguments after `--`. Set the same switches
	# the command line would, before the scene is added, so this process
	# never opens Bluetooth and never reads the owner's settings.
	var settings_dir := "/tmp/penguin-pass-smoke-%d" % OS.get_process_id()
	DirAccess.make_dir_recursive_absolute(settings_dir)
	var settings_path := settings_dir.path_join("settings.toml")
	var settings_file := FileAccess.open(settings_path, FileAccess.WRITE)
	if settings_file == null:
		_fail("could not write the temp settings file")
		return
	settings_file.store_string("this is not toml [[[\n")
	settings_file.close()
	OS.set_environment("PENGUIN_PASS_SETTINGS", settings_path)
	_settings_path = settings_path
	OS.set_environment("PENGUIN_PASS_FAKE_TRAINER", "1")
	OS.set_environment("PENGUIN_PASS_DASHBOARD_PORT", "0")
	OS.set_environment("PENGUIN_PASS_DASHBOARD_LOOPBACK", "1")
	OS.set_environment("PENGUIN_PASS_NO_MUSIC", "1")
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
var _settings_path := ""


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
	if int(game.debug_ready_ms()) > 5000:
		_fail("ready took %s ms" % game.debug_ready_ms())
		return
	if str(game.debug_screen()) != "title":
		_fail("launch screen is '%s', expected title" % game.debug_screen())
		return
	var held: float = game.distance_m()
	game.debug_advance(1.0)
	if float(game.distance_m()) > held + 0.05:
		_fail("the rider moved on the title screen")
		return
	if game.debug_music_tracks() < 1:
		_fail("music player has no tracks")
		return
	if str(game.debug_music_scene()) != "title":
		_fail("title music did not start: %s" % game.debug_music_scene())
		return
	if not _settings_hides(game):
		return
	game.debug_start_ride()
	if str(game.debug_screen()) != "ride":
		_fail("Start did not begin the ride: %s" % game.debug_screen())
		return
	if str(game.debug_music_scene()) != "ride":
		_fail("ride music did not start: %s" % game.debug_music_scene())
		return
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
	var rider_at: Vector3 = game.rider_position()
	var course_at: Vector3 = game.course_position()
	if rider_at.distance_to(course_at) > 1.5:
		_fail("rider is not on the course: %s vs %s" % [rider_at, course_at])
		return
	var grade_label := str(game.hud_grade_text())
	var grade_value := float(game.grade_pct())
	if abs(float(grade_label) - grade_value) > 0.15:
		_fail("grade label '%s' does not match %s" % [grade_label, grade_value])
		return
	if str(game.hud_elevation_text()) == "—":
		_fail("elevation should be a number")
		return
	if int(game.profile_point_count()) < 2:
		_fail("elevation profile is empty")
		return
	if int(game.lap()) < 1:
		_fail("lap should be at least 1 after moving")
		return
	game.debug_seek(game.course_length_m() + 40.0, 400.0, 0.0)
	if int(game.lap()) < 2:
		_fail("a full lap should increment the lap counter, got %s" % game.lap())
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
	if str(game.theme_name()) != "alpine":
		_fail("theme name '%s'" % game.theme_name())
		return
	if game.theme_fell_back():
		_fail("alpine theme should load")
		return
	if not game.debug_bad_theme_falls_back():
		_fail("a broken theme was accepted")
		return
	if game.scatter_count() < 10:
		_fail("scatter placed too little: %s" % game.scatter_count())
		return
	if game.scatter_checksum() != game.debug_scatter_checksum_again():
		_fail("scatter checksum changed between runs")
		return
	if game.scatter_road_violations() != 0:
		_fail("scatter sits on the road: %s" % game.scatter_road_violations())
		return
	if game.wheel_angle() < 1.0:
		_fail("wheels did not turn: %s" % game.wheel_angle())
		return
	if game.crank_angle() < 0.5:
		_fail("cranks did not turn: %s" % game.crank_angle())
		return
	var crank_stopped: float = game.crank_angle()
	game.debug_set_target_power(0.0)
	game.debug_advance(0.4)
	if abs(float(game.crank_angle()) - crank_stopped) > 0.01:
		_fail("cranks moved while coasting")
		return
	if not game.unknown_kind_is_empty():
		_fail("unknown scatter kind resolved to a mesh")
		return
	var assets := str(game.asset_line())
	if not assets.begins_with("PENGUIN_PASS_ASSETS models="):
		_fail("asset line missing: %s" % assets)
		return
	if assets.find("textures=") < 0 or assets.find("sky=") < 0:
		_fail("asset line incomplete: %s" % assets)
		return
	if FileAccess.file_exists("res://assets/manifest.json") and game.asset_model_count() < 1:
		_fail("manifest present but no models parsed")
		return
	if str(game.settings_warning()).find("could not parse") < 0:
		_fail("corrupt settings did not warn: '%s'" % game.settings_warning())
		return
	if not _menu_and_trainer(game):
		return
	if not _end_and_course(game):
		return
	if game is Node:
		game.free()
	print("PENGUIN_PASS_SMOKE_OK %s" % version)
	quit(0)


func _menu_and_trainer(game: Node) -> bool:
	game.debug_menu_open()
	if not game.debug_menu_open_state():
		_fail("menu did not open")
		return false
	if str(game.debug_menu_title()) != "Settings":
		_fail("menu title '%s'" % game.debug_menu_title())
		return false
	game.debug_menu_action("down")
	game.debug_menu_action("down")
	game.debug_menu_action("left")
	if abs(float(game.debug_difficulty()) - 0.45) > 0.001:
		_fail("difficulty did not step to 45 percent: %s" % game.debug_difficulty())
		return false
	game.debug_menu_action("down")
	game.debug_menu_action("right")
	if abs(float(game.debug_rider_mass_kg()) - 75.5) > 0.01:
		_fail("rider mass did not step: %s" % game.debug_rider_mass_kg())
		return false
	game.debug_menu_close()
	if game.debug_menu_open_state():
		_fail("menu did not close")
		return false
	var saved := FileAccess.get_file_as_string(str(game.settings_file()))
	if saved.find("75.5") < 0:
		_fail("settings file did not store the new weight: %s" % saved)
		return false
	if abs(float(game.debug_reload_settings()) - 75.5) > 0.01:
		_fail("a second load did not read the weight back")
		return false
	game.debug_menu_open()
	game.debug_menu_action("accept")
	game.debug_advance(0.35)
	var found := str(game.debug_discovered())
	if found.find("Fake Neo 2T") < 0 or found.find("Fake Kickr") < 0:
		_fail("fake trainers were not listed: %s" % found)
		return false
	if not found.begins_with("Fake Neo 2T"):
		_fail("strongest trainer should be first: %s" % found)
		return false
	game.debug_menu_action("accept")
	game.debug_advance(0.4)
	if str(game.debug_status_line()).find("connected") < 0:
		_fail("fake trainer did not connect: %s" % game.debug_status_line())
		return false
	if str(game.hud_power_text()) != "210":
		_fail("trainer power did not reach the HUD: %s" % game.hud_power_text())
		return false
	if str(game.hud_heart_text()) != "142":
		_fail("trainer heart rate did not reach the HUD: %s" % game.hud_heart_text())
		return false
	var grade := float(game.grade_pct()) * float(game.debug_difficulty())
	var sent := float(game.debug_fake_grade())
	if abs(sent - grade) > 0.15:
		_fail("grade command %s is not road grade times difficulty %s" % [sent, grade])
		return false
	var commands: int = game.debug_fake_grade_count()
	game.debug_menu_action("back")
	game.debug_menu_action("down")
	game.debug_menu_action("accept")
	game.debug_advance(0.5)
	if int(game.debug_fake_grade_count()) != commands:
		_fail("grade commands continued while resistance was off")
		return false
	if game.debug_control_trainer():
		_fail("resistance toggle did not turn off")
		return false
	game.debug_menu_action("accept")
	game.debug_advance(0.5)
	if not game.debug_control_trainer():
		_fail("resistance toggle did not turn back on")
		return false
	game.debug_menu_action("up")
	game.debug_menu_action("accept")
	game.debug_menu_action("down")
	game.debug_menu_action("down")
	game.debug_menu_action("down")
	game.debug_menu_action("accept")
	game.debug_advance(0.2)
	if str(game.debug_status_line()).find("Simulated") < 0:
		_fail("disconnect did not return to the simulated rider: %s" % game.debug_status_line())
		return false
	var blocked := "/tmp/penguin-pass-smoke-dir-%d" % OS.get_process_id()
	DirAccess.make_dir_recursive_absolute(blocked)
	game.debug_set_settings_path(blocked)
	game.debug_menu_action("back")
	game.debug_menu_action("down")
	game.debug_menu_action("right")
	if str(game.debug_menu_detail()).find("Could not save") < 0:
		_fail("a failed save did not show a message: %s" % game.debug_menu_detail())
		return false
	game.debug_set_settings_path(_settings_path)
	game.debug_use_no_bluetooth()
	game.debug_menu_open()
	game.debug_menu_action("accept")
	if str(game.debug_status_line()).find("Bluetooth unavailable") < 0:
		_fail("no-bluetooth did not report unavailable: %s" % game.debug_status_line())
		return false
	var before: float = game.distance_m()
	game.debug_set_target_power(200.0)
	game.debug_advance(1.0)
	if float(game.distance_m()) <= before:
		_fail("ride stopped when Bluetooth was unavailable")
		return false
	return true


func _settings_hides(game: Node) -> bool:
	# Title → Settings → Escape. The panel node must hide, not only the state.
	game.debug_show_title()
	game.debug_menu_action("down")
	game.debug_menu_action("down")
	game.debug_menu_action("down")
	game.debug_menu_action("down")
	game.debug_menu_action("accept")
	if not game.debug_settings_visible():
		_fail("settings panel did not show from the title")
		return false
	var quiet: float = game.debug_music_bus_db()
	for _step in 11:
		game.debug_menu_action("down")
	game.debug_menu_action("right")
	if abs(float(game.debug_music_bus_db()) - quiet) < 0.05:
		_fail("music volume did not change the bus")
		return false
	game.debug_press_back()
	if game.debug_settings_visible():
		_fail("settings panel stayed visible after Escape from the title")
		return false
	if str(game.debug_screen()) != "title":
		_fail("Escape from settings did not return to the title")
		return false
	# Trainer panel, then Escape back through settings. Same back handler as B.
	game.debug_show_title()
	game.debug_menu_action("down")
	game.debug_menu_action("down")
	game.debug_menu_action("accept")
	if not game.debug_settings_visible():
		_fail("trainer panel did not show")
		return false
	game.debug_press_back()
	if not game.debug_settings_visible():
		_fail("leaving the trainer list should still show settings")
		return false
	game.debug_press_back()
	if game.debug_settings_visible():
		_fail("trainer panel stayed visible after leaving settings")
		return false
	game.debug_clear_trainer()
	# Ride → pause → Settings → the same back key.
	game.debug_show_title()
	game.debug_start_ride()
	game.debug_press_back()
	game.debug_menu_action("down")
	game.debug_menu_action("down")
	game.debug_menu_action("down")
	game.debug_menu_action("accept")
	if not game.debug_settings_visible():
		_fail("settings panel did not show from the pause menu")
		return false
	game.debug_press_back()
	if game.debug_settings_visible():
		_fail("settings panel stayed visible after Escape from a ride")
		return false
	if str(game.debug_screen()) != "pause":
		_fail("Escape from settings did not return to pause: %s" % game.debug_screen())
		return false
	game.debug_show_title()
	return true


func _end_and_course(game: Node) -> bool:
	game.debug_menu_close()
	game.debug_menu_action("back")
	if str(game.debug_screen()) != "pause":
		_fail("pause did not open: %s" % game.debug_screen())
		return false
	game.debug_menu_action("down")
	game.debug_menu_action("down")
	game.debug_menu_action("down")
	game.debug_menu_action("accept")
	if str(game.debug_menu_title()) != "Settings":
		_fail("settings did not open from pause: %s" % game.debug_menu_title())
		return false
	game.debug_menu_action("back")
	game.debug_menu_action("down")
	game.debug_menu_action("accept")
	game.debug_menu_action("right")
	game.debug_menu_action("accept")
	if str(game.debug_screen()) != "summary":
		_fail("end ride did not show the summary: %s" % game.debug_screen())
		return false
	game.debug_menu_action("down")
	game.debug_menu_action("accept")
	if str(game.debug_screen()) != "title":
		_fail("summary did not return to the title: %s" % game.debug_screen())
		return false
	var penguin: float = game.course_length_m()
	if not game.debug_load_course("rolling", 10, 3):
		_fail("generated course did not load")
		return false
	if abs(float(game.course_length_m()) - penguin) < 1.0:
		_fail("generated course length did not change")
		return false
	game.debug_menu_action("down")
	game.debug_menu_action("accept")
	if str(game.debug_screen()) != "course":
		_fail("course picker did not open: %s" % game.debug_screen())
		return false
	game.debug_menu_action("accept")
	if abs(float(game.course_length_m()) - penguin) > 1.0:
		_fail("picking Penguin Pass did not switch back")
		return false
	return true


func _fail(message: String) -> void:
	push_error(message)
	print("PENGUIN_PASS_SMOKE_FAIL %s" % message)
	quit(1)
