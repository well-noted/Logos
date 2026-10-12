//! A typed view of the `<widgets>` section (Phase 2, Part M), read from the element tree.
//!
//! Reading is tolerant: a missing or unreadable attribute takes the value NetLogo 7.0.4 would
//! show for a new widget and is listed in [`Widget::problems`], and an element Logos doesn't
//! know becomes [`WidgetData::Unknown`], so every widget in a file is listed and none is lost.
//! Writes go through [`Nlogox::set_attribute`] and [`Nlogox::set_text`], which replace only the
//! bytes they change (D-19).

use crate::{EditError, Element, Nlogox};

/// One widget, with its index in the `<widgets>` section.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Widget {
    pub index: usize,
    /// The element name as written (`slider`, `view`, ...).
    pub element: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    /// `sizeVersion="0"`: a widget converted from a model saved before NetLogo 6.4 that keeps
    /// its old size (D-18).
    pub old_size: bool,
    /// The label NetLogo shows, when the file sets one.
    pub display: Option<String>,
    pub data: WidgetData,
    /// Attributes that were missing or unreadable, with the value used instead.
    pub problems: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum AgentKind {
    Observer,
    Turtle,
    Patch,
    Link,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Pen {
    pub display: String,
    pub interval: f64,
    /// 0 line, 1 bar, 2 point.
    pub mode: i32,
    /// NetLogo stores colors as Java ARGB integers.
    pub color: i32,
    pub legend: bool,
    pub setup: String,
    pub update: String,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Choice {
    /// `string`, `double`, `boolean` or `list`.
    pub kind: String,
    /// The value as NetLogo code, ready to put after `set <variable>`: strings are quoted.
    pub code: String,
    /// The value as the chooser shows it.
    pub label: String,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(
    feature = "serde",
    serde(
        tag = "kind",
        rename_all = "camelCase",
        rename_all_fields = "camelCase"
    )
)]
pub enum WidgetData {
    View {
        min_pxcor: i32,
        max_pxcor: i32,
        min_pycor: i32,
        max_pycor: i32,
        patch_size: f64,
        wrap_x: bool,
        wrap_y: bool,
        font_size: i32,
        /// 0 continuous, 1 on ticks.
        update_mode: i32,
        show_tick_counter: bool,
        tick_counter_label: Option<String>,
        frame_rate: f64,
    },
    Button {
        source: String,
        forever: bool,
        agent_kind: AgentKind,
        action_key: Option<char>,
        disable_until_ticks: bool,
    },
    Slider {
        variable: String,
        /// Minimum, maximum and step are reporters in NetLogo, kept as written.
        min: String,
        max: String,
        step: String,
        value: f64,
        units: Option<String>,
        vertical: bool,
    },
    Switch {
        variable: String,
        on: bool,
    },
    Chooser {
        variable: String,
        choices: Vec<Choice>,
        current: usize,
    },
    Input {
        variable: String,
        /// `number`, `color`, `string`, `reporter` or `command`.
        input_type: String,
        value: String,
        multiline: bool,
    },
    Monitor {
        source: String,
        precision: i32,
        font_size: i32,
        units: Option<String>,
    },
    Plot {
        x_axis: Option<String>,
        y_axis: Option<String>,
        x_min: f64,
        x_max: f64,
        y_min: f64,
        y_max: f64,
        auto_plot_x: bool,
        auto_plot_y: bool,
        legend: bool,
        setup: String,
        update: String,
        pens: Vec<Pen>,
    },
    Output {
        font_size: i32,
    },
    Note {
        text: String,
        font_size: i32,
        markdown: bool,
        text_color_light: Option<i32>,
        text_color_dark: Option<i32>,
        background_light: Option<i32>,
        background_dark: Option<i32>,
    },
    /// An element NetLogo 7.0.4 doesn't read either (`view3d` and anything newer).
    Unknown,
}

impl WidgetData {
    /// The global variable the widget defines, for sliders, switches, choosers and input boxes.
    pub fn variable(&self) -> Option<&str> {
        match self {
            WidgetData::Slider { variable, .. }
            | WidgetData::Switch { variable, .. }
            | WidgetData::Chooser { variable, .. }
            | WidgetData::Input { variable, .. } => Some(variable),
            _ => None,
        }
    }
}

/// Reads attributes of one element, collecting problems.
struct Reader<'a> {
    file: &'a Nlogox,
    element: &'a Element,
    problems: Vec<String>,
}

impl Reader<'_> {
    fn opt(&self, name: &str) -> Option<String> {
        self.file.attribute_value(self.element, name)
    }

    fn string(&mut self, name: &str, default: &str) -> String {
        self.opt(name).unwrap_or_else(|| {
            self.problems
                .push(format!("missing {name}; using \"{default}\""));
            default.to_string()
        })
    }

    fn int(&mut self, name: &str, default: i32) -> i32 {
        match self.opt(name) {
            Some(v) => v.trim().parse().unwrap_or_else(|_| {
                self.problems
                    .push(format!("{name}=\"{v}\" is not an integer; using {default}"));
                default
            }),
            None => {
                self.problems
                    .push(format!("missing {name}; using {default}"));
                default
            }
        }
    }

    fn opt_int(&mut self, name: &str) -> Option<i32> {
        let v = self.opt(name)?;
        match v.trim().parse() {
            Ok(n) => Some(n),
            Err(_) => {
                self.problems
                    .push(format!("{name}=\"{v}\" is not an integer; ignored"));
                None
            }
        }
    }

    fn float(&mut self, name: &str, default: f64) -> f64 {
        match self.opt(name) {
            Some(v) => v.trim().parse().unwrap_or_else(|_| {
                self.problems
                    .push(format!("{name}=\"{v}\" is not a number; using {default}"));
                default
            }),
            None => {
                self.problems
                    .push(format!("missing {name}; using {default}"));
                default
            }
        }
    }

    fn boolean(&mut self, name: &str, default: bool) -> bool {
        match self.opt(name).as_deref() {
            Some("true") => true,
            Some("false") => false,
            Some(v) => {
                self.problems.push(format!(
                    "{name}=\"{v}\" is not true or false; using {default}"
                ));
                default
            }
            None => {
                self.problems
                    .push(format!("missing {name}; using {default}"));
                default
            }
        }
    }

    fn text_of(&mut self, e: &Element) -> String {
        self.file.element_text(e).unwrap_or_else(|err| {
            self.problems
                .push(format!("<{}> text can't be read: {err}", e.name));
            String::new()
        })
    }

    fn child_text(&mut self, e: &Element, name: &str) -> String {
        match e.child(name) {
            Some(c) => self.text_of(c),
            None => String::new(),
        }
    }
}

/// Quotes a string as a NetLogo string literal.
pub fn netlogo_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn read_widget(file: &Nlogox, index: usize, e: &Element) -> Widget {
    let mut r = Reader {
        file,
        element: e,
        problems: Vec::new(),
    };
    let x = r.int("x", 0);
    let y = r.int("y", 0);
    let width = r.int("width", 0);
    let height = r.int("height", 0);
    let old_size = r.opt("sizeVersion").as_deref() == Some("0");
    let display = r.opt("display");
    let data = match e.name.as_str() {
        "view" => WidgetData::View {
            min_pxcor: r.int("minPxcor", -16),
            max_pxcor: r.int("maxPxcor", 16),
            min_pycor: r.int("minPycor", -16),
            max_pycor: r.int("maxPycor", 16),
            patch_size: r.float("patchSize", 13.0),
            wrap_x: r.boolean("wrappingAllowedX", true),
            wrap_y: r.boolean("wrappingAllowedY", true),
            font_size: r.int("fontSize", 10),
            update_mode: r.int("updateMode", 1),
            show_tick_counter: r.boolean("showTickCounter", true),
            tick_counter_label: r.opt("tickCounterLabel"),
            frame_rate: r.float("frameRate", 30.0),
        },
        "button" => {
            let agent_kind = match r.opt("kind").as_deref() {
                Some("Observer") | None => AgentKind::Observer,
                Some("Turtle") => AgentKind::Turtle,
                Some("Patch") => AgentKind::Patch,
                Some("Link") => AgentKind::Link,
                Some(other) => {
                    r.problems
                        .push(format!("kind=\"{other}\" is unknown; using Observer"));
                    AgentKind::Observer
                }
            };
            WidgetData::Button {
                source: r.text_of(e),
                forever: r.boolean("forever", false),
                agent_kind,
                action_key: r.opt("actionKey").and_then(|k| k.chars().next()),
                disable_until_ticks: r.boolean("disableUntilTicks", false),
            }
        }
        "slider" => WidgetData::Slider {
            variable: r.string("variable", ""),
            min: r.string("min", "0"),
            max: r.string("max", "100"),
            step: r.string("step", "1"),
            value: r.float("default", 50.0),
            units: r.opt("units"),
            vertical: r.opt("direction").as_deref() == Some("Vertical"),
        },
        "switch" => WidgetData::Switch {
            variable: r.string("variable", ""),
            on: r.boolean("on", false),
        },
        "chooser" => {
            let choices = e
                .elements()
                .filter(|c| c.name == "choice")
                .map(|c| {
                    let kind = file
                        .attribute_value(c, "type")
                        .unwrap_or_else(|| "string".into());
                    let value = file.attribute_value(c, "value").unwrap_or_default();
                    match kind.as_str() {
                        "string" => Choice {
                            code: netlogo_string(&value),
                            label: value,
                            kind,
                        },
                        "list" => {
                            let text = r.text_of(c);
                            Choice {
                                code: text.clone(),
                                label: text,
                                kind,
                            }
                        }
                        _ => Choice {
                            code: value.clone(),
                            label: value,
                            kind,
                        },
                    }
                })
                .collect();
            WidgetData::Chooser {
                variable: r.string("variable", ""),
                choices,
                current: r.int("current", 0).max(0) as usize,
            }
        }
        "input" => WidgetData::Input {
            variable: r.string("variable", ""),
            input_type: r.string("type", "string"),
            value: file.element_text(e).unwrap_or_default(),
            multiline: r.opt("multiline").as_deref() == Some("true"),
        },
        "monitor" => WidgetData::Monitor {
            source: r.text_of(e),
            precision: r.int("precision", 17),
            font_size: r.int("fontSize", 11),
            units: r.opt("units"),
        },
        "plot" => {
            let pens = e
                .elements()
                .filter(|p| p.name == "pen")
                .map(|p| {
                    let mut pr = Reader {
                        file,
                        element: p,
                        problems: Vec::new(),
                    };
                    let pen = Pen {
                        display: pr.string("display", "default"),
                        interval: pr.float("interval", 1.0),
                        mode: pr.int("mode", 0),
                        color: pr.int("color", -16777216),
                        legend: pr.boolean("legend", true),
                        setup: pr.child_text(p, "setup"),
                        update: pr.child_text(p, "update"),
                    };
                    r.problems
                        .extend(pr.problems.into_iter().map(|m| format!("pen: {m}")));
                    pen
                })
                .collect();
            WidgetData::Plot {
                x_axis: r.opt("xAxis"),
                y_axis: r.opt("yAxis"),
                x_min: r.float("xMin", 0.0),
                x_max: r.float("xMax", 10.0),
                y_min: r.float("yMin", 0.0),
                y_max: r.float("yMax", 10.0),
                auto_plot_x: r.boolean("autoPlotX", true),
                auto_plot_y: r.boolean("autoPlotY", true),
                legend: r.boolean("legend", false),
                setup: r.child_text(e, "setup"),
                update: r.child_text(e, "update"),
                pens,
            }
        }
        "output" => WidgetData::Output {
            font_size: r.int("fontSize", 12),
        },
        "note" => WidgetData::Note {
            text: r.text_of(e),
            font_size: r.int("fontSize", 12),
            markdown: r.opt("markdown").as_deref() == Some("true"),
            text_color_light: r.opt_int("textColorLight"),
            text_color_dark: r.opt_int("textColorDark"),
            background_light: r.opt_int("backgroundLight"),
            background_dark: r.opt_int("backgroundDark"),
        },
        _ => WidgetData::Unknown,
    };
    Widget {
        index,
        element: e.name.clone(),
        x,
        y,
        width,
        height,
        old_size,
        display,
        data,
        problems: r.problems,
    }
}

/// Formats a number the way NetLogo writes slider values (`34.0`, `0.155`).
fn format_double(v: f64) -> String {
    if v.is_finite() && v == v.trunc() && v.abs() < 1e15 {
        format!("{v:.1}")
    } else {
        format!("{v}")
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum WidgetEditError {
    #[error("no widget {0}")]
    NoSuchWidget(usize),
    #[error("widget {index} is a {element}, which has no {what}")]
    WrongKind {
        index: usize,
        element: String,
        what: &'static str,
    },
    #[error("choice {choice} is out of range for widget {index}")]
    NoSuchChoice { index: usize, choice: usize },
    #[error(transparent)]
    Edit(#[from] EditError),
}

impl Nlogox {
    /// Every widget in the `<widgets>` section, in file order.
    pub fn typed_widgets(&self) -> Vec<Widget> {
        self.widgets()
            .into_iter()
            .enumerate()
            .map(|(i, e)| read_widget(self, i, e))
            .collect()
    }

    /// One widget.
    pub fn typed_widget(&self, index: usize) -> Option<Widget> {
        self.widgets()
            .get(index)
            .map(|e| read_widget(self, index, e))
    }

    fn widget_element_path(&self, index: usize) -> Result<Vec<usize>, WidgetEditError> {
        self.widget_path(index)
            .ok_or(WidgetEditError::NoSuchWidget(index))
    }

    /// Moves and resizes a widget.
    pub fn set_widget_bounds(
        &mut self,
        index: usize,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    ) -> Result<(), WidgetEditError> {
        let path = self.widget_element_path(index)?;
        for (name, value) in [("x", x), ("y", y), ("width", width), ("height", height)] {
            self.set_attribute(&path, name, &value.to_string())?;
        }
        Ok(())
    }

    /// Stores a widget's current value, as NetLogo does when it saves a model: a slider's
    /// `default`, a switch's `on`, a chooser's `current`, an input box's text.
    pub fn set_widget_value(
        &mut self,
        index: usize,
        value: &WidgetValue,
    ) -> Result<(), WidgetEditError> {
        let w = self
            .typed_widget(index)
            .ok_or(WidgetEditError::NoSuchWidget(index))?;
        let path = self.widget_element_path(index)?;
        let wrong = |what| WidgetEditError::WrongKind {
            index,
            element: w.element.clone(),
            what,
        };
        match (&w.data, value) {
            (WidgetData::Slider { .. }, WidgetValue::Number(v)) => {
                self.set_attribute(&path, "default", &format_double(*v))?
            }
            (WidgetData::Switch { .. }, WidgetValue::Bool(on)) => {
                self.set_attribute(&path, "on", if *on { "true" } else { "false" })?
            }
            (WidgetData::Chooser { choices, .. }, WidgetValue::Choice(i)) => {
                if *i >= choices.len() {
                    return Err(WidgetEditError::NoSuchChoice { index, choice: *i });
                }
                self.set_attribute(&path, "current", &i.to_string())?
            }
            (WidgetData::Input { .. }, WidgetValue::Text(t)) => self.set_text(&path, t)?,
            (_, WidgetValue::Number(_)) => return Err(wrong("number value")),
            (_, WidgetValue::Bool(_)) => return Err(wrong("on/off value")),
            (_, WidgetValue::Choice(_)) => return Err(wrong("choice")),
            (_, WidgetValue::Text(_)) => return Err(wrong("text value")),
        }
        Ok(())
    }
}

/// The XML of a new widget of `kind` at (x, y), with the sizes and defaults Logos uses for new
/// widgets. `name` is the variable for sliders, switches, choosers and input boxes, and the
/// commands or reporter for buttons and monitors. Attributes are in the order NetLogo 7.0.4
/// writes them in the files it saves.
pub fn new_widget_xml(kind: &str, x: i32, y: i32, name: &str) -> Option<String> {
    let a = |s: &str| crate::text::encode_attribute(s, b'"');
    let t = |s: &str| crate::text::encode_content(s, "\n");
    Some(match kind {
        "button" => format!(
            r#"<button x="{x}" y="{y}" height="40" disableUntilTicks="false" forever="false" kind="Observer" width="90">{}</button>"#,
            t(name)
        ),
        "slider" => format!(
            r#"<slider x="{x}" step="1" y="{y}" max="100" width="170" display="{n}" height="50" min="0" direction="Horizontal" default="50.0" variable="{n}"></slider>"#,
            n = a(name)
        ),
        "switch" => format!(
            r#"<switch x="{x}" y="{y}" height="40" on="false" variable="{n}" width="150" display="{n}"></switch>"#,
            n = a(name)
        ),
        "chooser" => format!(
            r#"<chooser x="{x}" y="{y}" height="60" variable="{n}" current="0" width="150" display="{n}">
  <choice type="string" value="first"></choice>
  <choice type="string" value="second"></choice>
</chooser>"#,
            n = a(name)
        ),
        "input" => format!(
            r#"<input x="{x}" y="{y}" height="60" variable="{n}" multiline="false" width="170" type="string"></input>"#,
            n = a(name)
        ),
        "monitor" => format!(
            r#"<monitor x="{x}" precision="17" y="{y}" height="60" fontSize="11" width="100">{}</monitor>"#,
            t(name)
        ),
        "plot" => format!(
            r#"<plot x="{x}" autoPlotX="true" yMax="10.0" autoPlotY="true" y="{y}" xMin="0.0" height="160" legend="false" xMax="10.0" yMin="0.0" width="230" display="{n}">
  <setup></setup>
  <update></update>
  <pen interval="1.0" mode="0" display="default" color="-16777216" legend="true">
    <setup></setup>
    <update></update>
  </pen>
</plot>"#,
            n = a(name)
        ),
        "output" => {
            format!(r#"<output x="{x}" y="{y}" height="120" fontSize="12" width="250"></output>"#)
        }
        "note" => format!(
            r#"<note x="{x}" y="{y}" height="40" fontSize="12" width="150" markdown="false">{}</note>"#,
            t(name)
        ),
        _ => return None,
    })
}

impl Nlogox {
    /// Adds a widget at the end of the `<widgets>` section and returns its index.
    pub fn add_widget(&mut self, xml: &str) -> Result<usize, WidgetEditError> {
        let path = self
            .section_path("widgets")
            .ok_or(EditError::NoSection("widgets".into()))?;
        // Indent the lines inside a multi-line element like the widgets around it.
        let indent = self
            .widgets()
            .first()
            .and_then(|w| {
                let start = w.span.start;
                let line = self.text()[..start].rfind('\n').map_or(0, |i| i + 1);
                let lead = &self.text()[line..start];
                lead.chars()
                    .all(char::is_whitespace)
                    .then(|| lead.to_string())
            })
            .unwrap_or_else(|| "    ".into());
        let xml = xml.replace('\n', &format!("\n{indent}"));
        self.insert_element(&path, None, &xml)?;
        Ok(self.widgets().len() - 1)
    }

    /// Removes a widget.
    pub fn delete_widget(&mut self, index: usize) -> Result<(), WidgetEditError> {
        let path = self.widget_element_path(index)?;
        self.remove_element(&path)?;
        Ok(())
    }

    /// Sets one attribute of a widget, as written in the file (`display`, `min`, `forever`).
    pub fn set_widget_attribute(
        &mut self,
        index: usize,
        name: &str,
        value: &str,
    ) -> Result<(), WidgetEditError> {
        let path = self.widget_element_path(index)?;
        self.set_attribute(&path, name, value)?;
        Ok(())
    }

    /// Sets the text of a widget: a button's commands, a monitor's reporter, a note's text.
    pub fn set_widget_text(&mut self, index: usize, text: &str) -> Result<(), WidgetEditError> {
        let path = self.widget_element_path(index)?;
        self.set_text(&path, text)?;
        Ok(())
    }
}

/// A value set from the Interface tab.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize))]
#[cfg_attr(
    feature = "serde",
    serde(tag = "kind", content = "value", rename_all = "camelCase")
)]
pub enum WidgetValue {
    Number(f64),
    Bool(bool),
    Choice(usize),
    Text(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<model version="NetLogo 7.0.4" snapToGrid="false">
  <code><![CDATA[to setup end]]></code>
  <widgets>
    <view x="480" wrappingAllowedX="true" y="10" frameRate="30.0" minPycor="0" height="484" showTickCounter="true" patchSize="4.0" fontSize="10" wrappingAllowedY="true" width="324" tickCounterLabel="year" maxPycor="119" updateMode="1" maxPxcor="79" minPxcor="0"></view>
    <slider x="15" step="0.001" y="65" max="0.2" width="181" display="fertility" height="50" min="0" direction="Horizontal" default="0.155" variable="fertility"></slider>
    <button x="210" y="70" height="35" disableUntilTicks="true" forever="true" kind="Observer" width="90">go</button>
    <switch x="305" y="75" height="40" on="true" variable="historic-view?" width="147" display="historic-view?"></switch>
    <chooser x="310" y="120" height="60" variable="map-view" current="1" width="147" display="map-view">
      <choice type="string" value="zones"></choice>
      <choice type="double" value="2.5"></choice>
      <choice type="list">[1 2]</choice>
    </chooser>
    <monitor x="210" precision="3" y="180" height="60" fontSize="11" width="100" display="sites available">farm-sites-available</monitor>
    <plot x="20" autoPlotX="true" yMax="10.0" autoPlotY="true" y="465" xMin="0.0" height="260" legend="true" xMax="10.0" yMin="0.0" width="455" display="Population">
      <setup></setup>
      <update></update>
      <pen interval="1.0" mode="0" display="sim" color="-2674135" legend="true">
        <setup></setup>
        <update><![CDATA[ifelse x > 1 [ plot 1 ] [ plot 0 ]]]></update>
      </pen>
    </plot>
    <input x="8" y="996" width="208" height="80" variable="prompt" multiline="true" type="string">Hello &amp; welcome</input>
    <note x="1" y="2" width="3" height="4" fontSize="12" markdown="true">**bold**</note>
    <output x="1" y="2" width="3" height="4" fontSize="12"></output>
    <view3d x="0" y="0" width="1" height="1"></view3d>
  </widgets>
  <info></info>
</model>
"#;

    #[test]
    fn reads_every_kind() {
        let f = Nlogox::parse(FILE).unwrap();
        let w = f.typed_widgets();
        assert_eq!(w.len(), 11);
        assert!(w.iter().all(|w| w.problems.is_empty()), "{w:#?}");
        assert!(matches!(
            w[0].data,
            WidgetData::View {
                max_pxcor: 79,
                max_pycor: 119,
                ..
            }
        ));
        assert_eq!(
            w[1].data,
            WidgetData::Slider {
                variable: "fertility".into(),
                min: "0".into(),
                max: "0.2".into(),
                step: "0.001".into(),
                value: 0.155,
                units: None,
                vertical: false,
            }
        );
        assert!(
            matches!(&w[2].data, WidgetData::Button { source, forever: true, .. } if source == "go")
        );
        let WidgetData::Chooser {
            choices, current, ..
        } = &w[4].data
        else {
            panic!()
        };
        assert_eq!(*current, 1);
        assert_eq!(choices[0].code, "\"zones\"");
        assert_eq!(choices[1].code, "2.5");
        assert_eq!(choices[2].code, "[1 2]");
        assert!(
            matches!(&w[5].data, WidgetData::Monitor { source, precision: 3, .. } if source == "farm-sites-available")
        );
        assert_eq!(w[5].display.as_deref(), Some("sites available"));
        let WidgetData::Plot { pens, .. } = &w[6].data else {
            panic!()
        };
        assert_eq!(pens[0].update, "ifelse x > 1 [ plot 1 ] [ plot 0 ]");
        assert_eq!(pens[0].color, -2674135);
        assert!(
            matches!(&w[7].data, WidgetData::Input { value, multiline: true, .. } if value == "Hello & welcome")
        );
        assert!(matches!(
            &w[8].data,
            WidgetData::Note { markdown: true, .. }
        ));
        assert!(matches!(w[9].data, WidgetData::Output { font_size: 12 }));
        assert_eq!(w[10].data, WidgetData::Unknown);
    }

    #[test]
    fn missing_attributes_are_reported() {
        let f = Nlogox::parse(
            "<model><widgets><slider x=\"a\" variable=\"v\"></slider></widgets></model>",
        )
        .unwrap();
        let w = f.typed_widget(0).unwrap();
        assert!(w.problems.iter().any(|p| p.starts_with("x=\"a\"")));
        assert!(w.problems.iter().any(|p| p.starts_with("missing default")));
    }

    #[test]
    fn value_edits_change_only_their_bytes() {
        let mut f = Nlogox::parse(FILE).unwrap();
        f.set_widget_value(1, &WidgetValue::Number(0.2)).unwrap();
        f.set_widget_value(3, &WidgetValue::Bool(false)).unwrap();
        f.set_widget_value(4, &WidgetValue::Choice(2)).unwrap();
        f.set_widget_value(7, &WidgetValue::Text("a < b".into()))
            .unwrap();
        let expected = FILE
            .replace("default=\"0.155\"", "default=\"0.2\"")
            .replace(
                "on=\"true\" variable=\"historic-view?\"",
                "on=\"false\" variable=\"historic-view?\"",
            )
            .replace("current=\"1\"", "current=\"2\"")
            .replace("Hello &amp; welcome", "<![CDATA[a < b]]>");
        assert_eq!(f.text(), expected);
        assert!(matches!(
            f.set_widget_value(4, &WidgetValue::Choice(3)),
            Err(WidgetEditError::NoSuchChoice { .. })
        ));
        assert!(matches!(
            f.set_widget_value(2, &WidgetValue::Bool(true)),
            Err(WidgetEditError::WrongKind { .. })
        ));
    }

    #[test]
    fn whole_numbers_are_written_like_netlogo() {
        let mut f = Nlogox::parse(FILE).unwrap();
        f.set_widget_value(1, &WidgetValue::Number(34.0)).unwrap();
        assert!(f.text().contains("default=\"34.0\""));
    }

    #[test]
    fn bounds() {
        let mut f = Nlogox::parse(FILE).unwrap();
        f.set_widget_bounds(2, 1, 2, 3, 4).unwrap();
        let w = f.typed_widget(2).unwrap();
        assert_eq!((w.x, w.y, w.width, w.height), (1, 2, 3, 4));
    }

    #[test]
    fn aa4_reads_without_problems() {
        let text = include_str!("../../../corpus/own/aa4-combined/AA4-Combined.nlogox");
        let f = Nlogox::parse(text).unwrap();
        let w = f.typed_widgets();
        assert_eq!(w.len(), f.widgets().len());
        for w in &w {
            assert!(w.problems.is_empty(), "{w:#?}");
            assert_ne!(w.data, WidgetData::Unknown);
        }
        assert_eq!(
            w.iter().filter(|w| w.data.variable().is_some()).count(),
            w.iter()
                .filter(|w| matches!(
                    w.element.as_str(),
                    "slider" | "switch" | "chooser" | "input"
                ))
                .count()
        );
    }

    #[test]
    fn add_and_delete_widgets_keep_the_rest() {
        let mut f = Nlogox::parse(FILE).unwrap();
        let n = f.widgets().len();
        for kind in [
            "button", "slider", "switch", "chooser", "input", "monitor", "plot", "output", "note",
        ] {
            let xml = new_widget_xml(kind, 10, 20, "new-thing").unwrap();
            let i = f.add_widget(&xml).unwrap();
            let w = f.typed_widget(i).unwrap();
            assert!(w.problems.is_empty(), "{kind}: {w:?}");
            assert_ne!(w.data, WidgetData::Unknown, "{kind}");
            assert_eq!((w.x, w.y), (10, 20));
        }
        assert_eq!(f.widgets().len(), n + 9);
        // The new lines are indented like the others.
        assert!(f.text().contains("\n    <button x=\"10\""));
        assert!(
            f.text()
                .contains("\n      <choice type=\"string\" value=\"first\">")
        );
        for _ in 0..9 {
            f.delete_widget(n).unwrap();
        }
        assert_eq!(f.text(), FILE);
    }

    #[test]
    fn widget_text_and_attributes() {
        let mut f = Nlogox::parse(FILE).unwrap();
        f.set_widget_text(2, "go\nif ticks > 5 [ stop ]").unwrap();
        f.set_widget_attribute(2, "display", "Run <fast>").unwrap();
        let w = f.typed_widget(2).unwrap();
        assert_eq!(w.display.as_deref(), Some("Run <fast>"));
        assert!(
            matches!(&w.data, WidgetData::Button { source, .. } if source == "go\nif ticks > 5 [ stop ]")
        );
        assert!(f.delete_widget(99).is_err());
    }

    #[test]
    fn crlf_files_get_crlf() {
        let crlf = FILE.replace('\n', "\r\n");
        let mut f = Nlogox::parse(crlf.clone()).unwrap();
        let i = f
            .add_widget(&new_widget_xml("chooser", 1, 1, "c").unwrap())
            .unwrap();
        assert!(!f.text().replace("\r\n", "").contains('\n'));
        f.delete_widget(i).unwrap();
        assert_eq!(f.text(), crlf);
    }

    #[test]
    fn netlogo_strings() {
        assert_eq!(netlogo_string("a\"b\\c\n"), "\"a\\\"b\\\\c\\n\"");
    }
}
