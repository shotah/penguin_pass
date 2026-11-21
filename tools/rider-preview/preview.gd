extends SceneTree

## Render the pp-rider model to PNG contact sheets.
##
## User args (after --):
##   --glb-dir DIR   where render.sh put the exported .glb files
##   --out-dir DIR   where the PNGs go
##   --repo DIR      repository root, for the scenery models and the kept
##                   screenshot of the placeholder rider
##   --only A,B      render only these jobs (default: all)
##
## Jobs: turnaround, chase, first, strip, style, closeups, compare, and
## arraymesh, steer, stand, and tuck when their model files exist.
##
## The Godot flatpak has a private /tmp, so every path must be inside the
## repository. Models are loaded at run time with GLTFDocument; nothing
## goes through the import pipeline. Everything renders in a SubViewport,
## so a picture is the requested size whatever the window is.

const SETTLE_FRAMES := 6
const CHASE_POS := Vector3(0.7, 1.48, 3.05)
const CHASE_LOOK := Vector3(0.0, 1.05, -8.0)
const CHASE_FOV := 50.0
const FIRST_POS := Vector3(0.0, 1.22, -0.18)
const FIRST_LOOK := Vector3(0.0, 0.55, -6.0)
const FIRST_FOV := 72.0
const NATURE := "godot/assets/third_party/quaternius-stylized-nature/models"

var _glb_dir := ""
var _out_dir := ""
var _repo := ""
var _only := PackedStringArray()
var _jobs: Array = []
var _job := 0
var _tile := 0
var _settle := 0
var _seen := 0
var _sheet: Image
var _viewport: SubViewport
var _camera: Camera3D
var _label: Label
var _stage: Node3D
var _studio: Node3D
var _road: Node3D
var _post: Node3D
var _props: Node3D
var _models := {}
var _hidden: Array = []
var _failed := false


func _init() -> void:
	_read_args()
	_build_scene()
	_jobs = _make_jobs()
	if _jobs.is_empty():
		if _only.has("compare"):
			# Nothing to draw: compare only joins two pictures.
			_finish()
			return
		push_error("no jobs to render")
		quit(1)
		return
	RenderingServer.frame_post_draw.connect(_on_drawn)


func _read_args() -> void:
	var args := OS.get_cmdline_user_args()
	_glb_dir = _arg(args, "--glb-dir", "")
	_out_dir = _arg(args, "--out-dir", "")
	_repo = _arg(args, "--repo", "")
	var only := _arg(args, "--only", "")
	if not only.is_empty():
		_only = only.split(",", false)


func _arg(args: PackedStringArray, name: String, fallback: String) -> String:
	var index := args.find(name)
	if index >= 0 and index + 1 < args.size():
		return args[index + 1]
	return fallback


func _flat(color: Color, roughness: float) -> StandardMaterial3D:
	var material := StandardMaterial3D.new()
	material.albedo_color = color
	material.roughness = roughness
	return material


func _plane(size: Vector2, color: Color, at: Vector3) -> MeshInstance3D:
	var mesh := PlaneMesh.new()
	mesh.size = size
	var node := MeshInstance3D.new()
	node.mesh = mesh
	node.material_override = _flat(color, 1.0)
	node.position = at
	return node


func _build_scene() -> void:
	_viewport = SubViewport.new()
	_viewport.name = "PreviewViewport"
	_viewport.size = Vector2i(640, 640)
	_viewport.own_world_3d = true
	_viewport.transparent_bg = false
	_viewport.render_target_update_mode = SubViewport.UPDATE_ALWAYS
	_viewport.msaa_3d = Viewport.MSAA_4X
	root.add_child(_viewport)

	# Soft sky light, so shadowed sides keep their colour.
	var sky_material := ProceduralSkyMaterial.new()
	sky_material.sky_top_color = Color(0.26, 0.45, 0.80)
	sky_material.sky_horizon_color = Color(0.80, 0.85, 0.90)
	sky_material.ground_horizon_color = Color(0.62, 0.66, 0.62)
	sky_material.ground_bottom_color = Color(0.42, 0.46, 0.42)
	var sky := Sky.new()
	sky.sky_material = sky_material
	var env := Environment.new()
	env.background_mode = Environment.BG_SKY
	env.sky = sky
	env.ambient_light_source = Environment.AMBIENT_SOURCE_SKY
	env.ambient_light_energy = 1.0
	env.tonemap_mode = Environment.TONE_MAPPER_FILMIC
	var world_env := WorldEnvironment.new()
	world_env.environment = env
	_viewport.add_child(world_env)

	# The sun comes from behind the rider's right shoulder, so the side the
	# chase camera sees is lit.
	var sun := DirectionalLight3D.new()
	sun.rotation_degrees = Vector3(-42.0, 30.0, 0.0)
	sun.light_energy = 1.25
	sun.light_color = Color(1.0, 0.95, 0.86)
	sun.shadow_enabled = true
	sun.directional_shadow_max_distance = 40.0
	_viewport.add_child(sun)

	_studio = _plane(Vector2(80, 80), Color(0.60, 0.62, 0.56), Vector3.ZERO)
	_viewport.add_child(_studio)

	# A stand-in for the game's road: 6 m of asphalt with a dashed centre
	# line and edge lines, between grass verges.
	_road = Node3D.new()
	_road.add_child(_plane(Vector2(120, 120), Color(0.30, 0.48, 0.20), Vector3(0, -0.004, 0)))
	_road.add_child(_plane(Vector2(6, 120), Color(0.25, 0.25, 0.27), Vector3(0, -0.002, 0)))
	for side in [-1.0, 1.0]:
		_road.add_child(
			_plane(Vector2(0.12, 120), Color(0.85, 0.85, 0.82), Vector3(side * 2.75, -0.001, 0))
		)
	for i in range(-20, 21):
		_road.add_child(
			_plane(Vector2(0.12, 1.5), Color(0.85, 0.85, 0.82), Vector3(-1.5, -0.001, i * 3.0))
		)
	_viewport.add_child(_road)

	# 1.8 m reference post.
	_post = Node3D.new()
	var shaft := CylinderMesh.new()
	shaft.top_radius = 0.03
	shaft.bottom_radius = 0.03
	shaft.height = 1.8
	var shaft_node := MeshInstance3D.new()
	shaft_node.mesh = shaft
	shaft_node.material_override = _flat(Color(0.90, 0.33, 0.12), 0.8)
	shaft_node.position = Vector3(0, 0.9, 0)
	_post.add_child(shaft_node)
	var ball := SphereMesh.new()
	ball.radius = 0.05
	ball.height = 0.1
	var ball_node := MeshInstance3D.new()
	ball_node.mesh = ball
	ball_node.material_override = _flat(Color(0.90, 0.33, 0.12), 0.8)
	ball_node.position = Vector3(0, 1.8, 0)
	_post.add_child(ball_node)
	_post.position = Vector3(-0.85, 0.0, 0.55)
	_viewport.add_child(_post)

	_props = Node3D.new()
	_viewport.add_child(_props)

	_stage = Node3D.new()
	_viewport.add_child(_stage)

	_camera = Camera3D.new()
	_camera.near = 0.05
	_camera.far = 400.0
	_camera.current = true
	_viewport.add_child(_camera)

	_label = Label.new()
	_label.position = Vector2(14, 10)
	var settings := LabelSettings.new()
	settings.font_size = 22
	settings.font_color = Color(1, 1, 1)
	settings.outline_size = 6
	settings.outline_color = Color(0.1, 0.1, 0.12)
	_label.label_settings = settings
	_viewport.add_child(_label)


func _load(path: String) -> Node3D:
	if _models.has(path):
		return _models[path]
	if not FileAccess.file_exists(path):
		push_error("missing model %s" % path)
		_failed = true
		return null
	var document := GLTFDocument.new()
	var state := GLTFState.new()
	var err := document.append_from_file(path, state)
	if err != OK:
		push_error("cannot read %s (error %d)" % [path, err])
		_failed = true
		return null
	var scene := document.generate_scene(state) as Node3D
	if scene == null:
		push_error("no scene in %s" % path)
		_failed = true
		return null
	_models[path] = scene
	return scene


## Build the model from plain arrays, exactly as docs/rider.md tells the
## game to: one ArrayMesh surface per material with clockwise indices, a
## StandardMaterial3D from the sRGB colour, and the four-column transform.
func _load_arrays(path: String) -> Node3D:
	if _models.has(path):
		return _models[path]
	var data = JSON.parse_string(FileAccess.get_file_as_string(path))
	if typeof(data) != TYPE_DICTIONARY:
		push_error("cannot read %s" % path)
		_failed = true
		return null
	var scene := Node3D.new()
	for part in data["parts"]:
		var mesh := ArrayMesh.new()
		var count := 0
		for surface in part["surfaces"]:
			var positions := PackedVector3Array()
			var normals := PackedVector3Array()
			var flat: Array = surface["positions"]
			var flat_normals: Array = surface["normals"]
			for i in range(0, flat.size(), 3):
				positions.append(Vector3(flat[i], flat[i + 1], flat[i + 2]))
				normals.append(Vector3(flat_normals[i], flat_normals[i + 1], flat_normals[i + 2]))
			var indices := PackedInt32Array()
			for index in surface["indices_clockwise"]:
				indices.append(int(index))
			var arrays := []
			arrays.resize(Mesh.ARRAY_MAX)
			arrays[Mesh.ARRAY_VERTEX] = positions
			arrays[Mesh.ARRAY_NORMAL] = normals
			arrays[Mesh.ARRAY_INDEX] = indices
			mesh.add_surface_from_arrays(Mesh.PRIMITIVE_TRIANGLES, arrays)
			var color: Array = surface["color"]
			var material := StandardMaterial3D.new()
			material.albedo_color = Color(color[0], color[1], color[2])
			material.roughness = surface["roughness"]
			material.metallic = surface["metallic"]
			mesh.surface_set_material(count, material)
			count += 1
		var node := MeshInstance3D.new()
		node.name = part["name"]
		node.mesh = mesh
		var t: Array = part["transform"]
		node.transform = Transform3D(
			Basis(
				Vector3(t[0][0], t[0][1], t[0][2]),
				Vector3(t[1][0], t[1][1], t[1][2]),
				Vector3(t[2][0], t[2][1], t[2][2])
			),
			Vector3(t[3][0], t[3][1], t[3][2])
		)
		scene.add_child(node)
	_models[path] = scene
	return scene


func _rider(file: String) -> Node3D:
	var path := _glb_dir.path_join(file)
	var known := _models.has(path)
	var scene: Node3D
	if file.ends_with(".json"):
		scene = _load_arrays(path)
	else:
		scene = _load(path)
	if scene != null and not known:
		scene.visible = false
		_stage.add_child(scene)
	return scene


func _add_prop(file: String, at: Vector3, yaw_deg: float, scale: float) -> void:
	var path := _repo.path_join(NATURE).path_join(file)
	var document := GLTFDocument.new()
	var state := GLTFState.new()
	if document.append_from_file(path, state) != OK:
		push_error("cannot read scenery model %s" % path)
		return
	var scene := document.generate_scene(state) as Node3D
	if scene == null:
		return
	scene.position = at
	scene.rotation_degrees = Vector3(0, yaw_deg, 0)
	scene.scale = Vector3.ONE * scale
	_props.add_child(scene)


func _has(file: String) -> bool:
	return FileAccess.file_exists(_glb_dir.path_join(file))


func _wanted(name: String) -> bool:
	return _only.is_empty() or _only.has(name)


func _ortho(label: String, glb: String, pos: Vector3, target: Vector3, size: float) -> Dictionary:
	return {"label": label, "glb": glb, "pos": pos, "target": target, "ortho": size}


func _persp(label: String, glb: String, pos: Vector3, target: Vector3, fov: float) -> Dictionary:
	return {"label": label, "glb": glb, "pos": pos, "target": target, "fov": fov}


func _make_jobs() -> Array:
	var jobs: Array = []
	var mid := Vector3(0.0, 0.86, -0.09)

	if _wanted("turnaround"):
		var g := "rest.glb"
		var top := _ortho("top", g, Vector3(0, 8, -0.09), Vector3(0, 0, -0.09), 2.0)
		top["up"] = Vector3(0, 0, -1)
		jobs.append({
			"name": "turnaround",
			"cols": 4,
			"tile": Vector2i(640, 640),
			"tiles": [
				_ortho("right side", g, mid + Vector3(8, 0, 0), mid, 2.0),
				_ortho("left side", g, mid + Vector3(-8, 0, 0), mid, 2.0),
				_ortho("front", g, mid + Vector3(0, 0, -8), mid, 2.0),
				_ortho("rear", g, mid + Vector3(0, 0, 8), mid, 2.0),
				_persp("three-quarter front", g, Vector3(2.6, 1.45, -3.3), mid, 28.0),
				_persp("three-quarter rear", g, Vector3(2.6, 1.45, 3.3), mid, 28.0),
				top,
				_persp("three-quarter front, left", g, Vector3(-2.6, 1.45, -3.3), mid, 28.0),
			],
		})

	if _wanted("chase"):
		var tile := _persp("", "chase.glb", CHASE_POS, CHASE_LOOK, CHASE_FOV)
		tile["ground"] = "road"
		tile["post"] = false
		jobs.append({"name": "chase", "cols": 1, "tile": Vector2i(1280, 800), "tiles": [tile]})

	if _wanted("first"):
		# The game's handlebar camera, as camera.rs places it.
		var bars := _persp("", "chase.glb", FIRST_POS, FIRST_LOOK, FIRST_FOV)
		bars["ground"] = "road"
		bars["post"] = false
		bars["near"] = 0.08
		jobs.append({"name": "first-person", "cols": 1, "tile": Vector2i(1280, 800), "tiles": [bars]})
		# From the eye anchor, looking down over the bars. The head is
		# hidden, as a first-person view would hide it.
		var eye := _persp("", "chase.glb", Vector3.ZERO, Vector3.ZERO, FIRST_FOV)
		eye["anchor"] = "anchor_eye"
		eye["look_dir"] = Vector3(0.0, -1.20, -1.0)
		eye["hide"] = ["head"]
		eye["ground"] = "road"
		eye["post"] = false
		eye["near"] = 0.08
		jobs.append({"name": "first-person-eye", "cols": 1, "tile": Vector2i(1280, 800), "tiles": [eye]})

	if _wanted("strip"):
		var tiles: Array = []
		for row in 2:
			for i in 8:
				var deg := i * 45
				var g := "strip-%03d.glb" % deg
				var tile: Dictionary
				if row == 0:
					tile = _ortho("crank %d" % deg, g, mid + Vector3(8, 0, 0), mid, 1.9)
				else:
					tile = _ortho("", g, mid + Vector3(0, 0, 8), mid, 1.9)
				tile["post"] = false
				tiles.append(tile)
		jobs.append({"name": "pedal-stroke", "cols": 8, "tile": Vector2i(440, 440), "tiles": tiles})

	if _wanted("style"):
		_add_prop("pine_5.gltf", Vector3(-3.4, 0, -3.6), 20.0, 1.0)
		_add_prop("rock_medium_1.gltf", Vector3(2.9, 0, -3.4), 40.0, 0.8)
		_add_prop("bush_common_flowers.gltf", Vector3(-2.3, 0, 0.5), 0.0, 1.0)
		_add_prop("fern_1.gltf", Vector3(1.9, 0, -0.9), 70.0, 1.0)
		var tile := _persp(
			"", "rest.glb", Vector3(3.3, 1.5, 4.3), Vector3(-0.35, 1.05, -0.6), 40.0
		)
		tile["post"] = false
		tile["props"] = true
		tile["ground"] = "grass"
		jobs.append({"name": "style-check", "cols": 1, "tile": Vector2i(1280, 800), "tiles": [tile]})

	if _wanted("closeups"):
		var g := "chase.glb"
		var b := "bike.glb"
		var tiles := [
			_persp("drivetrain", b, Vector3(1.15, 0.42, 0.55), Vector3(0.02, 0.30, 0.20), 30.0),
			_persp("cockpit", b, Vector3(0.30, 1.36, 0.02), Vector3(0.0, 0.90, -0.55), 40.0),
			_persp("hands on the hoods", g, Vector3(0.75, 1.38, -1.20), Vector3(0.04, 0.95, -0.50), 28.0),
			_persp("helmet, rear", g, Vector3(0.42, 1.74, 0.40), Vector3(0.0, 1.59, -0.28), 34.0),
			_persp("face", g, Vector3(0.40, 1.56, -1.05), Vector3(0.0, 1.57, -0.32), 32.0),
			_persp("legs and shoes, rear", g, Vector3(0.50, 0.70, 1.60), Vector3(0.0, 0.45, 0.05), 34.0),
		]
		for tile in tiles:
			tile["post"] = false
		jobs.append({"name": "closeups", "cols": 3, "tile": Vector2i(800, 700), "tiles": tiles})

	if _wanted("arraymesh") and _has("chase.json"):
		var tiles: Array = []
		for pair in [["glTF route", "chase.glb"], ["ArrayMesh recipe", "chase.json"]]:
			var chase := _persp(pair[0], pair[1], CHASE_POS, CHASE_LOOK, CHASE_FOV)
			chase["ground"] = "road"
			chase["post"] = false
			tiles.append(chase)
			var front := _persp("", pair[1], Vector3(2.6, 1.45, -3.3), mid, 28.0)
			front["post"] = false
			tiles.append(front)
		jobs.append(
			{"name": "arraymesh-check", "cols": 2, "tile": Vector2i(640, 480), "tiles": tiles}
		)

	if _wanted("steer") and _has("steer.glb"):
		var g := "steer.glb"
		var top := _ortho("top", g, Vector3(0, 8, -0.3), Vector3(0, 0, -0.3), 1.5)
		top["up"] = Vector3(0, 0, -1)
		var chase := _persp("chase", g, CHASE_POS, CHASE_LOOK, CHASE_FOV)
		chase["ground"] = "road"
		var tiles := [
			top,
			_persp("three-quarter front", g, Vector3(2.6, 1.45, -3.3), mid, 28.0),
			_ortho("front", g, mid + Vector3(0, 0, -8), mid, 2.0),
			chase,
		]
		for tile in tiles:
			tile["post"] = false
		jobs.append({"name": "steer", "cols": 4, "tile": Vector2i(640, 640), "tiles": tiles})

	for extra in ["stand", "tuck"]:
		if _wanted(extra) and _has("%s.glb" % extra):
			var g := "%s.glb" % extra
			var chase := _persp("chase", g, CHASE_POS, CHASE_LOOK, CHASE_FOV)
			chase["ground"] = "road"
			var tiles := [
				_ortho("right side", g, mid + Vector3(8, 0, 0), mid, 2.0),
				_ortho("rear", g, mid + Vector3(0, 0, 8), mid, 2.0),
				_persp("three-quarter front", g, Vector3(2.6, 1.45, -3.3), mid, 28.0),
				chase,
			]
			for tile in tiles:
				tile["post"] = false
			jobs.append({"name": extra, "cols": 4, "tile": Vector2i(640, 640), "tiles": tiles})

	return jobs


func _setup_tile(job: Dictionary, tile: Dictionary) -> void:
	_viewport.size = job["tile"]
	for node in _hidden:
		node.visible = true
	_hidden.clear()
	for model in _models.values():
		model.visible = false
	var model := _rider(tile["glb"])
	if model != null:
		model.visible = true
		for name in tile.get("hide", []):
			var node := model.find_child(name, true, false) as Node3D
			if node != null:
				node.visible = false
				_hidden.append(node)

	var ground: String = tile.get("ground", "studio")
	_road.visible = ground == "road"
	_studio.visible = ground != "road"
	var tint := Color(0.60, 0.62, 0.56)
	if ground == "grass":
		tint = Color(0.36, 0.50, 0.26)
	(_studio as MeshInstance3D).material_override = _flat(tint, 1.0)
	_post.visible = tile.get("post", true)
	_props.visible = tile.get("props", false)

	var pos: Vector3 = tile["pos"]
	if tile.has("anchor") and model != null:
		var anchor := model.find_child(tile["anchor"], true, false) as Node3D
		if anchor != null:
			pos = anchor.global_position
		else:
			push_error("no node %s in %s" % [tile["anchor"], tile["glb"]])
			_failed = true
	_camera.near = tile.get("near", 0.05)
	if tile.has("ortho"):
		_camera.projection = Camera3D.PROJECTION_ORTHOGONAL
		_camera.size = tile["ortho"]
	else:
		_camera.projection = Camera3D.PROJECTION_PERSPECTIVE
		_camera.fov = tile["fov"]
	var target: Vector3 = tile["target"]
	if tile.has("look_dir"):
		target = pos + tile["look_dir"]
	_camera.look_at_from_position(pos, target, tile.get("up", Vector3.UP))
	_label.text = tile.get("label", "")


func _on_drawn() -> void:
	_seen += 1
	if _seen < 4:
		return
	if _job >= _jobs.size():
		_finish()
		return
	var job: Dictionary = _jobs[_job]
	var tiles: Array = job["tiles"]
	if _settle == 0:
		_setup_tile(job, tiles[_tile])
		_settle = 1
		return
	_settle += 1
	if _settle < SETTLE_FRAMES:
		return
	_settle = 0

	var size: Vector2i = job["tile"]
	var cols: int = job["cols"]
	var image := _viewport.get_texture().get_image()
	if image == null or image.get_width() != size.x or image.get_height() != size.y:
		push_error("viewport image was not %dx%d" % [size.x, size.y])
		_failed = true
		_finish()
		return
	if _tile == 0:
		var rows := int(ceil(float(tiles.size()) / float(cols)))
		_sheet = Image.create(size.x * cols, size.y * rows, false, image.get_format())
		_sheet.fill(Color(0.14, 0.15, 0.17))
	var cell := Vector2i(_tile % cols, _tile / cols)
	_sheet.blit_rect(image, Rect2i(Vector2i.ZERO, size), cell * size)
	_tile += 1
	if _tile < tiles.size():
		return
	_save(_sheet, "%s.png" % job["name"])
	_tile = 0
	_job += 1


func _save(image: Image, file: String) -> void:
	DirAccess.make_dir_recursive_absolute(_out_dir)
	var path := _out_dir.path_join(file)
	var err := image.save_png(path)
	if err != OK:
		push_error("save_png %s failed: %d" % [path, err])
		_failed = true
		return
	print("RIDER_PREVIEW %s %dx%d" % [path, image.get_width(), image.get_height()])


## Put the placeholder rider's chase screenshot next to the new one.
func _compare() -> void:
	# A copy of the game's start-chase shot from before the new model, kept
	# so this picture still shows the placeholder after the game changes.
	var before_path := _repo.path_join("docs/img/rider/history/placeholder-start-chase.png")
	var after_path := _out_dir.path_join("chase.png")
	if not FileAccess.file_exists(before_path) or not FileAccess.file_exists(after_path):
		push_error("compare needs %s and %s" % [before_path, after_path])
		_failed = true
		return
	var before := Image.load_from_file(before_path)
	var after := Image.load_from_file(after_path)
	if before == null or after == null:
		_failed = true
		return
	var size := after.get_size()
	before.convert(after.get_format())
	if before.get_size() != size:
		before.resize(size.x, size.y, Image.INTERPOLATE_LANCZOS)
	var sheet := Image.create(size.x * 2 + 16, size.y, false, after.get_format())
	sheet.fill(Color(0.14, 0.15, 0.17))
	sheet.blit_rect(before, Rect2i(Vector2i.ZERO, size), Vector2i.ZERO)
	sheet.blit_rect(after, Rect2i(Vector2i.ZERO, size), Vector2i(size.x + 16, 0))
	_save(sheet, "chase-compare.png")


func _finish() -> void:
	if RenderingServer.frame_post_draw.is_connected(_on_drawn):
		RenderingServer.frame_post_draw.disconnect(_on_drawn)
	if _wanted("compare") and not _failed:
		_compare()
	if _failed:
		print("RIDER_PREVIEW_FAILED")
		quit(1)
		return
	print("RIDER_PREVIEW_DONE")
	quit(0)
