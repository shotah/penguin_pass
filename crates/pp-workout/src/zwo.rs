//! Import a Zwift `.zwo` file.
//!
//! The file is XML. [`parse_zwo`] uses `roxmltree` and does not contain a
//! parser of its own. An unknown element is skipped and named in the warning
//! list. A missing or unreadable attribute skips that step. Neither fails the
//! rest of the file. A file that is not XML, or whose root is not
//! `workout_file`, is an error.

use roxmltree::Node;

use crate::library::LoadWarning;
use crate::model::{Cue, IntervalStep, OpenStep, RampStep, SteadyStep, Step, Workout};

/// One file, parsed. Warnings did not drop the workout.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedZwo {
    /// The plan. The id is the file name without `.zwo`.
    pub workout: Workout,
    /// Elements and attributes that were skipped.
    pub warnings: Vec<LoadWarning>,
}

/// A file that could not be read as a workout at all.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ZwoError {
    /// The bytes are not XML.
    #[error("malformed xml: {0}")]
    Xml(String),
    /// The root element is not `workout_file`.
    #[error("root element is not workout_file")]
    NotAWorkout,
}

/// Parse one `.zwo` document.
///
/// `file_name` is the file's own name, such as `sample.zwo`. It becomes the
/// workout id and is copied onto each warning. Power attributes are fractions
/// of FTP, the same way the file stores them (`0.75`, not `75`).
///
/// # Errors
///
/// [`ZwoError::Xml`] when the document is not XML. [`ZwoError::NotAWorkout`]
/// when the root element has another name. Unknown steps, missing attributes,
/// and a missing `workout` element are warnings on an otherwise empty or
/// partial [`ParsedZwo`].
pub fn parse_zwo(file_name: &str, xml: &str) -> Result<ParsedZwo, ZwoError> {
    let xml = xml.trim_start_matches('\u{feff}');
    let document = roxmltree::Document::parse(xml).map_err(|err| ZwoError::Xml(err.to_string()))?;
    let root = document.root_element();
    if root.tag_name().name() != "workout_file" {
        return Err(ZwoError::NotAWorkout);
    }
    let mut warnings = Vec::new();
    let mut name = String::new();
    let mut description = String::new();
    let mut steps = Vec::new();
    let mut saw_workout = false;
    for child in elements(root) {
        match child.tag_name().name() {
            "author" | "sportType" | "sporttype" | "category" | "subcategory" | "durationType"
            | "createdby" | "createdBy" | "tags" => {}
            "name" => name = direct_text(child),
            "description" => description = direct_text(child),
            "workout" => {
                if saw_workout {
                    warn(&mut warnings, file_name, "skipped extra workout element");
                } else {
                    saw_workout = true;
                    read_workout(file_name, child, &mut steps, &mut warnings);
                }
            }
            other => warn(
                &mut warnings,
                file_name,
                format!("skipped unknown element {other}"),
            ),
        }
    }
    if !saw_workout {
        warn(&mut warnings, file_name, "missing workout element");
    }
    let id = file_stem(file_name);
    if name.is_empty() {
        name = id.clone();
    }
    Ok(ParsedZwo {
        workout: Workout {
            id,
            name,
            description,
            steps,
            ramp_test: false,
        },
        warnings,
    })
}

fn read_workout(
    file: &str,
    workout: Node<'_, '_>,
    steps: &mut Vec<Step>,
    warnings: &mut Vec<LoadWarning>,
) {
    for child in elements(workout) {
        match child.tag_name().name() {
            "Warmup" => push_ramp(file, child, warnings, steps, RampKind::Warmup),
            "Cooldown" => push_ramp(file, child, warnings, steps, RampKind::Cooldown),
            "Ramp" => push_ramp(file, child, warnings, steps, RampKind::Ramp),
            "SteadyState" => push_steady(file, child, warnings, steps),
            "IntervalsT" => push_intervals(file, child, warnings, steps),
            "FreeRide" => push_open(file, child, warnings, steps, false),
            "MaxEffort" => push_open(file, child, warnings, steps, true),
            "textevent" => warn(warnings, file, "skipped textevent outside a step"),
            other => warn(warnings, file, format!("skipped unknown element {other}")),
        }
    }
}

#[derive(Clone, Copy)]
enum RampKind {
    Warmup,
    Cooldown,
    Ramp,
}

fn push_ramp(
    file: &str,
    node: Node<'_, '_>,
    warnings: &mut Vec<LoadWarning>,
    steps: &mut Vec<Step>,
    kind: RampKind,
) {
    let Some(duration_s) = required_seconds(file, node, "Duration", warnings) else {
        return;
    };
    let Some(power_low) = required_fraction(file, node, "PowerLow", warnings) else {
        return;
    };
    let Some(power_high) = required_fraction(file, node, "PowerHigh", warnings) else {
        return;
    };
    let step = RampStep {
        duration_s,
        power_low,
        power_high,
        cadence_rpm: optional_cadence(file, node, "Cadence", warnings),
        cues: read_cues(file, node, warnings),
    };
    steps.push(match kind {
        RampKind::Warmup => Step::Warmup(step),
        RampKind::Cooldown => Step::Cooldown(step),
        RampKind::Ramp => Step::Ramp(step),
    });
}

fn push_steady(
    file: &str,
    node: Node<'_, '_>,
    warnings: &mut Vec<LoadWarning>,
    steps: &mut Vec<Step>,
) {
    let Some(duration_s) = required_seconds(file, node, "Duration", warnings) else {
        return;
    };
    let power = if let Some(power) = optional_fraction(node, "Power") {
        power
    } else if let (Some(low), Some(high)) = (
        optional_fraction(node, "PowerLow"),
        optional_fraction(node, "PowerHigh"),
    ) {
        if (low - high).abs() < 1e-6 {
            low
        } else {
            // A steady element with two ends is a ramp in all but name.
            steps.push(Step::Ramp(RampStep {
                duration_s,
                power_low: low,
                power_high: high,
                cadence_rpm: optional_cadence(file, node, "Cadence", warnings),
                cues: read_cues(file, node, warnings),
            }));
            return;
        }
    } else {
        warn(warnings, file, "skipped SteadyState: missing Power");
        return;
    };
    steps.push(Step::Steady(SteadyStep {
        duration_s,
        power,
        cadence_rpm: optional_cadence(file, node, "Cadence", warnings),
        cues: read_cues(file, node, warnings),
        label: None,
    }));
}

fn push_intervals(
    file: &str,
    node: Node<'_, '_>,
    warnings: &mut Vec<LoadWarning>,
    steps: &mut Vec<Step>,
) {
    let Some(repeat) = required_repeat(file, node, warnings) else {
        return;
    };
    let Some(on_s) = required_seconds(file, node, "OnDuration", warnings) else {
        return;
    };
    let Some(off_s) = required_seconds(file, node, "OffDuration", warnings) else {
        return;
    };
    let Some(on_power) = required_fraction(file, node, "OnPower", warnings) else {
        return;
    };
    let Some(off_power) = required_fraction(file, node, "OffPower", warnings) else {
        return;
    };
    steps.push(Step::Intervals(IntervalStep {
        repeat,
        on_s,
        off_s,
        on_power,
        off_power,
        on_cadence: optional_cadence(file, node, "Cadence", warnings),
        off_cadence: optional_cadence(file, node, "CadenceResting", warnings),
        cues: read_cues(file, node, warnings),
        on_label: None,
        off_label: None,
    }));
}

fn push_open(
    file: &str,
    node: Node<'_, '_>,
    warnings: &mut Vec<LoadWarning>,
    steps: &mut Vec<Step>,
    max_effort: bool,
) {
    let Some(duration_s) = required_seconds(file, node, "Duration", warnings) else {
        return;
    };
    let step = OpenStep {
        duration_s,
        cues: read_cues(file, node, warnings),
        show_power: optional_fraction(node, "Power"),
    };
    steps.push(if max_effort {
        Step::MaxEffort(step)
    } else {
        Step::FreeRide(step)
    });
}

fn read_cues(file: &str, node: Node<'_, '_>, warnings: &mut Vec<LoadWarning>) -> Vec<Cue> {
    let mut cues = Vec::new();
    for child in elements(node) {
        match child.tag_name().name() {
            "textevent" => {
                let Some(text) = cue_text(child) else {
                    warn(warnings, file, "skipped textevent: missing message");
                    continue;
                };
                let offset_s = match child.attribute("timeoffset") {
                    None => 0.0,
                    Some(raw) => match parse_f32(raw) {
                        Some(seconds) if seconds >= 0.0 => seconds,
                        _ => {
                            warn(warnings, file, "skipped textevent: bad timeoffset");
                            continue;
                        }
                    },
                };
                cues.push(Cue { offset_s, text });
            }
            other => warn(
                warnings,
                file,
                format!("skipped child {other} of {}", node.tag_name().name()),
            ),
        }
    }
    cues
}

fn cue_text(node: Node<'_, '_>) -> Option<String> {
    if let Some(message) = node.attribute("message") {
        let text = message.trim();
        if !text.is_empty() {
            return Some(text.to_owned());
        }
    }
    let text = direct_text(node);
    if text.is_empty() { None } else { Some(text) }
}

fn required_seconds(
    file: &str,
    node: Node<'_, '_>,
    attribute: &str,
    warnings: &mut Vec<LoadWarning>,
) -> Option<f32> {
    required_number(file, node, attribute, warnings, true)
}

fn required_fraction(
    file: &str,
    node: Node<'_, '_>,
    attribute: &str,
    warnings: &mut Vec<LoadWarning>,
) -> Option<f32> {
    required_number(file, node, attribute, warnings, false)
}

fn required_number(
    file: &str,
    node: Node<'_, '_>,
    attribute: &str,
    warnings: &mut Vec<LoadWarning>,
    duration: bool,
) -> Option<f32> {
    let element = node.tag_name().name();
    let Some(raw) = node.attribute(attribute) else {
        warn(
            warnings,
            file,
            format!("skipped {element}: missing {attribute}"),
        );
        return None;
    };
    let Some(value) = parse_f32(raw) else {
        warn(
            warnings,
            file,
            format!("skipped {element}: {attribute} is not a number"),
        );
        return None;
    };
    if duration && value <= 0.0 {
        warn(
            warnings,
            file,
            format!("skipped {element}: {attribute} is not positive"),
        );
        return None;
    }
    if !duration && value < 0.0 {
        warn(
            warnings,
            file,
            format!("skipped {element}: {attribute} is negative"),
        );
        return None;
    }
    Some(value)
}

fn optional_fraction(node: Node<'_, '_>, attribute: &str) -> Option<f32> {
    let raw = node.attribute(attribute)?;
    let value = parse_f32(raw)?;
    (value >= 0.0).then_some(value)
}

fn required_repeat(file: &str, node: Node<'_, '_>, warnings: &mut Vec<LoadWarning>) -> Option<u16> {
    let Some(raw) = node.attribute("Repeat") else {
        warn(warnings, file, "skipped IntervalsT: missing Repeat");
        return None;
    };
    let repeat = raw.parse::<u16>().ok().filter(|repeat| *repeat > 0);
    if repeat.is_none() {
        warn(warnings, file, "skipped IntervalsT: Repeat is not a number");
    }
    repeat
}

fn optional_cadence(
    file: &str,
    node: Node<'_, '_>,
    attribute: &str,
    warnings: &mut Vec<LoadWarning>,
) -> Option<u16> {
    let raw = node.attribute(attribute)?;
    let Some(value) = parse_f32(raw) else {
        warn(
            warnings,
            file,
            format!("ignored {attribute} on {}", node.tag_name().name()),
        );
        return None;
    };
    if value <= 0.0 || value > 250.0 {
        warn(
            warnings,
            file,
            format!("ignored {attribute} on {}", node.tag_name().name()),
        );
        return None;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Some(value.round() as u16)
}

fn parse_f32(raw: &str) -> Option<f32> {
    let value = raw.trim().parse::<f32>().ok()?;
    value.is_finite().then_some(value)
}

fn elements<'doc, 'input>(node: Node<'doc, 'input>) -> Vec<Node<'doc, 'input>> {
    node.children().filter(Node::is_element).collect()
}

fn direct_text(node: Node<'_, '_>) -> String {
    let mut text = String::new();
    for child in node.children() {
        if let Some(piece) = child.text() {
            text.push_str(piece);
        }
    }
    text.trim().to_owned()
}

fn file_stem(file_name: &str) -> String {
    let stem = std::path::Path::new(file_name)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(file_name);
    if stem.is_empty() {
        "workout".to_owned()
    } else {
        stem.to_owned()
    }
}

fn warn(warnings: &mut Vec<LoadWarning>, file: &str, message: impl Into<String>) {
    warnings.push(LoadWarning {
        file: Some(file.to_owned()),
        message: message.into(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Step;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<workout_file>
  <author>Penguin Pass</author>
  <name>Sample</name>
  <description>A hand-written sample.</description>
  <sportType>bike</sportType>
  <tags><tag name="test"/></tags>
  <workout>
    <Warmup Duration="300" PowerLow="0.40" PowerHigh="0.70" Cadence="85">
      <textevent timeoffset="0" message="Spin easy"/>
      <textevent timeoffset="60" message="A little more"/>
      <note/>
    </Warmup>
    <SteadyState Duration="600" Power="0.90" Cadence="90">
      <textevent timeoffset="0" message="Hold"/>
    </SteadyState>
    <IntervalsT Repeat="3" OnDuration="30" OffDuration="30" OnPower="1.20" OffPower="0.50" Cadence="100" CadenceResting="80"/>
    <Ramp Duration="120" PowerLow="0.50" PowerHigh="0.90"/>
    <FreeRide Duration="180" FlatRoad="1"/>
    <MaxEffort Duration="20" Power="1.50"/>
    <Cooldown Duration="180" PowerLow="0.60" PowerHigh="0.40"/>
    <Mystery Duration="10" Power="0.5"/>
    <SteadyState Power="0.80"/>
    <SteadyState Duration="nope" Power="0.80"/>
  </workout>
</workout_file>
"#;

    #[test]
    fn a_sample_keeps_every_known_step_and_warns_on_the_rest() {
        let parsed = parse_zwo("sample.zwo", SAMPLE).expect("parse");
        assert_eq!(parsed.workout.id, "sample");
        assert_eq!(parsed.workout.name, "Sample");
        assert_eq!(parsed.workout.description, "A hand-written sample.");
        assert!(!parsed.workout.ramp_test);
        assert_eq!(parsed.workout.steps.len(), 7);
        assert!(matches!(parsed.workout.steps[0], Step::Warmup(_)));
        assert!(matches!(parsed.workout.steps[1], Step::Steady(_)));
        assert!(matches!(parsed.workout.steps[2], Step::Intervals(_)));
        assert!(matches!(parsed.workout.steps[3], Step::Ramp(_)));
        assert!(matches!(parsed.workout.steps[4], Step::FreeRide(_)));
        assert!(matches!(parsed.workout.steps[5], Step::MaxEffort(_)));
        assert!(matches!(parsed.workout.steps[6], Step::Cooldown(_)));
        // 300 + 600 + 3*60 + 120 + 180 + 20 + 180 = 1580.
        assert!((parsed.workout.duration_s() - 1580.0).abs() < 1e-3);
        let messages: Vec<_> = parsed
            .warnings
            .iter()
            .map(|warning| warning.message.as_str())
            .collect();
        assert!(messages.iter().any(|line| line.contains("note")));
        assert!(messages.iter().any(|line| line.contains("Mystery")));
        assert!(
            messages
                .iter()
                .any(|line| line.contains("missing Duration"))
        );
        assert!(
            messages
                .iter()
                .any(|line| line.contains("Duration is not a number"))
        );
        assert!(
            parsed
                .warnings
                .iter()
                .all(|warning| warning.file.as_deref() == Some("sample.zwo"))
        );
        let bars = parsed.workout.profile();
        assert!(bars.iter().any(|bar| bar.open && bar.label == "Free ride"));
        assert!(bars.iter().any(|bar| bar.label == "Hard 2 of 3"));
        assert!(parsed.workout.intensity().stress_score.is_none());
    }

    #[test]
    fn a_malformed_file_and_a_foreign_root_are_errors() {
        let broken = parse_zwo("bad.zwo", "<workout_file><workout></workout");
        assert!(matches!(broken, Err(ZwoError::Xml(_))));
        let foreign = parse_zwo("nope.zwo", "<foo/>");
        assert_eq!(foreign, Err(ZwoError::NotAWorkout));
    }

    #[test]
    fn a_missing_name_uses_the_file_stem() {
        let xml = "<workout_file><workout><FreeRide Duration=\"30\"/></workout></workout_file>";
        let parsed = parse_zwo("evening.zwo", xml).expect("parse");
        assert_eq!(parsed.workout.id, "evening");
        assert_eq!(parsed.workout.name, "evening");
        assert_eq!(parsed.warnings.len(), 0);
    }
}
