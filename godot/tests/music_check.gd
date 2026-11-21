extends SceneTree

## Headless check for MusicPlayer. A failed check quits non-zero.
## This script never opens Bluetooth and never loads the ride scene.

var _player: Node
var _phase := 0
var _frames := 0
var _title := ""
var _title_secs := 0.0
var _ride_count := 0
var _manifest_count := 0
var _heard: Array[String] = []
var _position_seen := 0.0


func _init() -> void:
	if not ClassDB.class_exists(&"MusicPlayer"):
		_fail("MusicPlayer is not registered")
		return
	var made = ClassDB.instantiate(&"MusicPlayer")
	if made == null or not (made is Node):
		_fail("MusicPlayer could not be instantiated")
		return
	_player = made
	root.add_child(_player)
	process_frame.connect(_on_frame)


func _on_frame() -> void:
	_frames += 1
	if _phase == 0:
		if _frames < 2:
			return
		_check_library()
		return
	if _phase == 1:
		_position_seen = max(_position_seen, _position())
		if _position_seen > 0.05 or _frames > 180:
			_check_title_playing()
		return
	if _phase == 2:
		_check_ride_rotation()
		return
	if _phase == 3:
		if _position() > 0.05 or _frames > 180:
			_check_volume_and_folder()
		return
	if _phase == 4 and _frames >= 20:
		_player.free()
		_phase = 5
		_frames = 0
		return
	if _phase == 5 and _frames >= 8:
		print("PENGUIN_PASS_MUSIC_OK tracks=%d" % _manifest_count)
		quit(0)


func _check_library() -> void:
	var text := FileAccess.get_file_as_string("res://assets/music/music.json")
	var parsed = JSON.parse_string(text)
	if typeof(parsed) != TYPE_DICTIONARY:
		_fail("music.json did not parse")
		return
	var tracks: Array = parsed["tracks"]
	_manifest_count = tracks.size()
	var titles := 0
	var rides := 0
	var worst_us := 0
	var worst_name := ""
	var worst_bytes := 0
	for entry in tracks:
		var file_name := str(entry["file"])
		var path := "res://assets/music/" + file_name
		var role := str(entry["role"])
		if role == "title":
			titles += 1
			_title = str(entry["title"])
		elif role == "ride":
			rides += 1
		else:
			_fail("unknown role %s" % role)
			return
		if not FileAccess.file_exists(path):
			_fail("missing %s" % file_name)
			return
		if file_name.ends_with(".ogg"):
			_title_secs = float(entry["duration_secs"])
			continue
		var decoded := _decode_file(path, float(entry["duration_secs"]))
		if decoded < 0:
			_fail("could not decode %s" % file_name)
			return
		var handle := FileAccess.open(path, FileAccess.READ)
		var size := 0
		if handle != null:
			size = handle.get_length()
			handle.close()
		if decoded > worst_us:
			worst_us = decoded
			worst_name = file_name
			worst_bytes = size
	if titles != 1:
		_fail("expected one title track, got %s" % titles)
		return
	if rides < 6 or rides > 8:
		_fail("expected 6 to 8 ride tracks, got %s" % rides)
		return
	if int(_player.track_count()) != rides:
		_fail("track_count %s is not the ride count %s" % [_player.track_count(), rides])
		return
	_ride_count = rides
	print("PENGUIN_PASS_MUSIC_LOAD_US %d bytes=%d file=%s" % [worst_us, worst_bytes, worst_name])
	_player.set_volume_percent(100)
	var bus := AudioServer.get_bus_index("Music")
	if bus < 0:
		_fail("Music bus is missing")
		return
	if abs(AudioServer.get_bus_volume_db(bus)) > 0.05:
		_fail("volume 100 is %s dB" % AudioServer.get_bus_volume_db(bus))
		return
	if AudioServer.is_bus_mute(bus):
		_fail("volume 100 muted the bus")
		return
	_player.set_scene("title")
	if str(_player.now_playing()) != _title:
		_fail("title scene plays '%s', expected '%s'" % [_player.now_playing(), _title])
		return
	_phase = 1
	_frames = 0
	_position_seen = 0.0


func _check_title_playing() -> void:
	if _position_seen <= 0.05:
		_fail("playback position did not advance (%s)" % _position_seen)
		return
	if abs(float(_player.live_length_secs()) - _title_secs) > 1.0:
		_fail("title length %s manifest %s" % [_player.live_length_secs(), _title_secs])
		return
	var before := _position()
	_player.set_scene("title")
	if str(_player.now_playing()) != _title:
		_fail("setting the title twice changed the track")
		return
	if _position() + 0.02 < before:
		_fail("setting the title twice restarted playback")
		return
	_player.set_scene("ride")
	if int(_player.track_count()) != _ride_count:
		_fail("ride track_count changed")
		return
	_phase = 2
	_frames = 0


func _check_ride_rotation() -> void:
	_heard = []
	for index in _ride_count:
		var name := str(_player.now_playing())
		if name.is_empty():
			_fail("ride track has no name")
			return
		if name == _title:
			_fail("ride scene is still playing the title")
			return
		if _heard.has(name):
			_fail("ride repeated %s inside one cycle" % name)
			return
		_heard.append(name)
		if index + 1 < _ride_count:
			_player.next_track()
	if _heard.size() != _ride_count:
		_fail("heard %s of %s ride tracks" % [_heard.size(), _ride_count])
		return
	var last := str(_player.now_playing())
	if _ride_count >= 2:
		_player.next_track()
		if str(_player.now_playing()) == last:
			_fail("the next cycle opened on %s" % last)
			return
	_phase = 3
	_frames = 0


func _check_volume_and_folder() -> void:
	if _position() <= 0.05 and _position_seen <= 0.05:
		_fail("ride audio did not decode")
		return
	var bus := AudioServer.get_bus_index("Music")
	_player.set_volume_percent(0)
	if not AudioServer.is_bus_mute(bus):
		_fail("volume 0 did not mute the Music bus")
		return
	for child in _player.get_children():
		if child is AudioStreamPlayer and child.playing and not child.stream_paused:
			_fail("volume 0 left a stream decoding")
			return
	_player.set_volume_percent(100)
	if AudioServer.is_bus_mute(bus):
		_fail("volume 100 left the bus muted")
		return
	var folder := "/tmp/penguin-pass-music-%d" % OS.get_process_id()
	DirAccess.make_dir_recursive_absolute(folder)
	var source := "res://assets/music/forever.mp3"
	var copy := FileAccess.open(folder + "/copied-track.mp3", FileAccess.WRITE)
	if copy == null:
		_fail("could not write the temp track")
		return
	copy.store_buffer(FileAccess.get_file_as_bytes(source))
	copy.close()
	var dummy := FileAccess.open(folder + "/skip-me.flac", FileAccess.WRITE)
	if dummy == null:
		_fail("could not write the flac dummy")
		return
	dummy.store_string("not audio")
	dummy.close()
	var report := str(_player.set_user_folder(folder))
	if not report.begins_with("1 track found"):
		_fail("folder report '%s'" % report)
		return
	if report.find("1 skipped (FLAC is not supported)") < 0:
		_fail("folder report did not name FLAC: '%s'" % report)
		return
	_player.set_scene("ride")
	if int(_player.track_count()) != 1:
		_fail("user folder track_count %s" % _player.track_count())
		return
	if str(_player.now_playing()) != "copied track":
		_fail("user folder now playing '%s'" % _player.now_playing())
		return
	_player.set_scene("title")
	if str(_player.now_playing()) != _title:
		_fail("user folder replaced the title track")
		return
	# Stop the streams, then let the audio server mix once before quit.
	# Freeing on the same frame leaves the Ogg playback in the leak report.
	print("PENGUIN_PASS_MUSIC_DECODE_US %d bytes=%d" % [_player.slowest_load_us(), _player.slowest_load_bytes()])
	# Let the title stream start, then free on a later frame.
	_phase = 4
	_frames = 0


func _decode_file(path: String, expected: float) -> int:
	var started := Time.get_ticks_usec()
	var stream: AudioStream = null
	if path.ends_with(".mp3"):
		stream = AudioStreamMP3.load_from_file(path)
	elif path.ends_with(".wav"):
		stream = AudioStreamWAV.load_from_file(path)
	var elapsed := int(Time.get_ticks_usec() - started)
	if stream == null:
		return -1
	if abs(stream.get_length() - expected) > 1.0:
		_fail("duration %s godot %s manifest %s" % [path, stream.get_length(), expected])
		return -1
	stream = null
	return elapsed


func _position() -> float:
	var best := 0.0
	for child in _player.get_children():
		if child is AudioStreamPlayer:
			best = max(best, child.get_playback_position())
	return best


func _fail(message: String) -> void:
	push_error(message)
	print("PENGUIN_PASS_MUSIC_FAIL %s" % message)
	quit(1)
