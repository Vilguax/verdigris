use crate::clientbound::write_nbt_network_string;
use crate::primitives::write_i32;

pub use crate::play::packet_ids::clientbound::SYSTEM_CHAT;

const TAG_END: u8 = 0x00;
const TAG_BYTE: u8 = 0x01;
const TAG_INT: u8 = 0x03;
const TAG_STRING: u8 = 0x08;
const TAG_LIST: u8 = 0x09;
const TAG_COMPOUND: u8 = 0x0A;
const TAG_INT_ARRAY: u8 = 0x0B;

#[derive(Debug, Clone, PartialEq)]
pub enum Content {
    Text(String),
    Translatable { key: String, args: Vec<Arg> },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Arg {
    Str(String),
    Int(i32),
    Comp(Box<Component>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClickEvent {
    pub action: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HoverEvent {
    ShowText(Box<Component>),
    ShowEntity {
        id: String,
        uuid: u128,
        name: Box<Component>,
    },
    ShowItem {
        id: String,
    },
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Style {
    pub color: Option<String>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underlined: Option<bool>,
    pub strikethrough: Option<bool>,
    pub obfuscated: Option<bool>,
    pub insertion: Option<String>,
    pub click: Option<ClickEvent>,
    pub hover: Option<HoverEvent>,
}

impl Style {
    pub fn is_empty(&self) -> bool {
        self.color.is_none()
            && self.bold.is_none()
            && self.italic.is_none()
            && self.underlined.is_none()
            && self.strikethrough.is_none()
            && self.obfuscated.is_none()
            && self.insertion.is_none()
            && self.click.is_none()
            && self.hover.is_none()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Component {
    pub content: Content,
    pub style: Style,
    pub extra: Vec<Component>,
}

impl Component {
    pub fn text(value: impl Into<String>) -> Self {
        Component {
            content: Content::Text(value.into()),
            style: Style::default(),
            extra: Vec::new(),
        }
    }

    pub fn translate(key: impl Into<String>) -> Self {
        Component {
            content: Content::Translatable {
                key: key.into(),
                args: Vec::new(),
            },
            style: Style::default(),
            extra: Vec::new(),
        }
    }

    pub fn color(mut self, color: impl Into<String>) -> Self {
        self.style.color = Some(color.into());
        self
    }

    pub fn with(mut self, arg: Arg) -> Self {
        if let Content::Translatable { args, .. } = &mut self.content {
            args.push(arg);
        }
        self
    }
}

pub fn root_tag_type(component: &Component) -> u8 {
    match &component.content {
        Content::Text(_) if component.style.is_empty() && component.extra.is_empty() => TAG_STRING,
        _ => TAG_COMPOUND,
    }
}

pub fn write_nbt(buf: &mut Vec<u8>, component: &Component) {
    buf.push(root_tag_type(component));
    write_component_payload(buf, component);
}

pub fn system_chat_payload(component: &Component, overlay: bool) -> Vec<u8> {
    let mut payload = Vec::new();
    write_nbt(&mut payload, component);
    payload.push(u8::from(overlay));
    payload
}

pub fn player_display_name(name: &str, uuid: u128) -> Component {
    Component {
        content: Content::Text(name.to_owned()),
        style: Style {
            insertion: Some(name.to_owned()),
            click: Some(ClickEvent {
                action: "suggest_command".to_owned(),
                value: format!("/tell {name} "),
            }),
            hover: Some(HoverEvent::ShowEntity {
                id: "minecraft:player".to_owned(),
                uuid,
                name: Box::new(Component::text(name)),
            }),
            ..Style::default()
        },
        extra: Vec::new(),
    }
}

fn write_component_payload(buf: &mut Vec<u8>, component: &Component) {
    if root_tag_type(component) == TAG_STRING {
        if let Content::Text(value) = &component.content {
            write_nbt_network_string(value, buf);
        }
        return;
    }

    match &component.content {
        Content::Text(value) => write_string_entry(buf, "text", value),
        Content::Translatable { key, .. } => write_string_entry(buf, "translate", key),
    }

    write_style(buf, &component.style);

    if let Content::Translatable { args, .. } = &component.content
        && !args.is_empty()
    {
        write_entry_header(buf, TAG_LIST, "with");
        write_arg_list(buf, args);
    }

    if !component.extra.is_empty() {
        write_entry_header(buf, TAG_LIST, "extra");
        write_component_list(buf, &component.extra);
    }

    buf.push(TAG_END);
}

fn write_style(buf: &mut Vec<u8>, style: &Style) {
    if let Some(color) = &style.color {
        write_string_entry(buf, "color", color);
    }
    write_bool_flag(buf, "bold", style.bold);
    write_bool_flag(buf, "italic", style.italic);
    write_bool_flag(buf, "underlined", style.underlined);
    write_bool_flag(buf, "strikethrough", style.strikethrough);
    write_bool_flag(buf, "obfuscated", style.obfuscated);
    if let Some(insertion) = &style.insertion {
        write_string_entry(buf, "insertion", insertion);
    }
    if let Some(click) = &style.click {
        write_entry_header(buf, TAG_COMPOUND, "click_event");
        write_click_event(buf, click);
    }
    if let Some(hover) = &style.hover {
        write_entry_header(buf, TAG_COMPOUND, "hover_event");
        write_hover_event(buf, hover);
    }
}

fn write_click_event(buf: &mut Vec<u8>, click: &ClickEvent) {
    write_string_entry(buf, "action", &click.action);
    let field = match click.action.as_str() {
        "suggest_command" | "run_command" => "command",
        "open_url" => "url",
        _ => "value",
    };
    write_string_entry(buf, field, &click.value);
    buf.push(TAG_END);
}

fn write_hover_event(buf: &mut Vec<u8>, hover: &HoverEvent) {
    match hover {
        HoverEvent::ShowText(component) => {
            write_string_entry(buf, "action", "show_text");
            write_entry_header(buf, root_tag_type(component), "value");
            write_component_payload(buf, component);
        }
        HoverEvent::ShowEntity { id, uuid, name } => {
            write_string_entry(buf, "action", "show_entity");
            write_string_entry(buf, "id", id);
            write_entry_header(buf, TAG_INT_ARRAY, "uuid");
            write_uuid_int_array(buf, *uuid);
            write_entry_header(buf, root_tag_type(name), "name");
            write_component_payload(buf, name);
        }
        HoverEvent::ShowItem { id } => {
            write_string_entry(buf, "action", "show_item");
            write_string_entry(buf, "id", id);
        }
    }
    buf.push(TAG_END);
}

fn write_uuid_int_array(buf: &mut Vec<u8>, uuid: u128) {
    write_i32(4, buf);
    write_i32((uuid >> 96) as i32, buf);
    write_i32((uuid >> 64) as i32, buf);
    write_i32((uuid >> 32) as i32, buf);
    write_i32(uuid as i32, buf);
}

enum ListItem<'a> {
    Str(&'a str),
    Int(i32),
    Comp(&'a Component),
}

impl ListItem<'_> {
    fn tag(&self) -> u8 {
        match self {
            ListItem::Str(_) => TAG_STRING,
            ListItem::Int(_) => TAG_INT,
            ListItem::Comp(component) => root_tag_type(component),
        }
    }

    fn write_payload(&self, buf: &mut Vec<u8>) {
        match self {
            ListItem::Str(value) => write_nbt_network_string(value, buf),
            ListItem::Int(value) => write_i32(*value, buf),
            ListItem::Comp(component) => write_component_payload(buf, component),
        }
    }
}

fn write_arg_list(buf: &mut Vec<u8>, args: &[Arg]) {
    let items: Vec<ListItem> = args
        .iter()
        .map(|arg| match arg {
            Arg::Str(value) => ListItem::Str(value),
            Arg::Int(value) => ListItem::Int(*value),
            Arg::Comp(component) => ListItem::Comp(component),
        })
        .collect();
    write_list(buf, &items);
}

fn write_component_list(buf: &mut Vec<u8>, components: &[Component]) {
    let items: Vec<ListItem> = components.iter().map(ListItem::Comp).collect();
    write_list(buf, &items);
}

fn write_list(buf: &mut Vec<u8>, items: &[ListItem]) {
    let first_tag = items.first().map_or(TAG_END, ListItem::tag);
    let homogeneous = items.iter().all(|item| item.tag() == first_tag);
    if homogeneous {
        buf.push(first_tag);
        write_i32(items.len() as i32, buf);
        for item in items {
            item.write_payload(buf);
        }
    } else {
        buf.push(TAG_COMPOUND);
        write_i32(items.len() as i32, buf);
        for item in items {
            if item.tag() == TAG_COMPOUND {
                item.write_payload(buf);
            } else {
                buf.push(item.tag());
                write_nbt_network_string("", buf);
                item.write_payload(buf);
                buf.push(TAG_END);
            }
        }
    }
}

fn write_entry_header(buf: &mut Vec<u8>, tag: u8, name: &str) {
    buf.push(tag);
    write_nbt_network_string(name, buf);
}

fn write_string_entry(buf: &mut Vec<u8>, name: &str, value: &str) {
    write_entry_header(buf, TAG_STRING, name);
    write_nbt_network_string(value, buf);
}

fn write_bool_flag(buf: &mut Vec<u8>, name: &str, flag: Option<bool>) {
    if flag == Some(true) {
        write_entry_header(buf, TAG_BYTE, name);
        buf.push(0x01);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nbt(component: &Component) -> Vec<u8> {
        let mut buf = Vec::new();
        write_nbt(&mut buf, component);
        buf
    }

    fn joined(name: &str) -> Component {
        Component::translate("multiplayer.player.joined")
            .color("yellow")
            .with(Arg::Str(name.to_owned()))
    }

    fn left(name: &str) -> Component {
        Component::translate("multiplayer.player.left")
            .color("yellow")
            .with(Arg::Str(name.to_owned()))
    }

    #[test]
    fn plain_text_falls_back_to_tag_string() {
        let component = Component::text("Bonjour");
        assert_eq!(root_tag_type(&component), TAG_STRING);
        assert_eq!(
            nbt(&component),
            vec![0x08, 0x00, 0x07, b'B', b'o', b'n', b'j', b'o', b'u', b'r']
        );
        assert_eq!(nbt(&component).len(), 10);
    }

    #[test]
    fn empty_style_and_extra_keeps_string_root() {
        let component = Component::text("x");
        assert_eq!(root_tag_type(&component), TAG_STRING);
        let styled = Component::text("x").color("white");
        assert_eq!(root_tag_type(&styled), TAG_COMPOUND);
    }

    #[test]
    fn joined_message_is_seventy_six_bytes() {
        let expected = vec![
            0x0A, 0x08, 0x00, 0x09, b't', b'r', b'a', b'n', b's', b'l', b'a', b't', b'e', 0x00,
            0x19, b'm', b'u', b'l', b't', b'i', b'p', b'l', b'a', b'y', b'e', b'r', b'.', b'p',
            b'l', b'a', b'y', b'e', b'r', b'.', b'j', b'o', b'i', b'n', b'e', b'd', 0x08, 0x00,
            0x05, b'c', b'o', b'l', b'o', b'r', 0x00, 0x06, b'y', b'e', b'l', b'l', b'o', b'w',
            0x09, 0x00, 0x04, b'w', b'i', b't', b'h', 0x08, 0x00, 0x00, 0x00, 0x01, 0x00, 0x05,
            b'A', b'l', b'i', b'c', b'e', 0x00,
        ];
        let payload = nbt(&joined("Alice"));
        assert_eq!(payload.len(), 76);
        assert_eq!(payload, expected);

        let mut body = expected.clone();
        body.push(0x00);
        assert_eq!(system_chat_payload(&joined("Alice"), false), body);
        assert_eq!(system_chat_payload(&joined("Alice"), false).len(), 77);
    }

    #[test]
    fn left_message_is_seventy_four_bytes() {
        let expected = vec![
            0x0A, 0x08, 0x00, 0x09, b't', b'r', b'a', b'n', b's', b'l', b'a', b't', b'e', 0x00,
            0x17, b'm', b'u', b'l', b't', b'i', b'p', b'l', b'a', b'y', b'e', b'r', b'.', b'p',
            b'l', b'a', b'y', b'e', b'r', b'.', b'l', b'e', b'f', b't', 0x08, 0x00, 0x05, b'c',
            b'o', b'l', b'o', b'r', 0x00, 0x06, b'y', b'e', b'l', b'l', b'o', b'w', 0x09, 0x00,
            0x04, b'w', b'i', b't', b'h', 0x08, 0x00, 0x00, 0x00, 0x01, 0x00, 0x05, b'A', b'l',
            b'i', b'c', b'e', 0x00,
        ];
        let payload = nbt(&left("Alice"));
        assert_eq!(payload.len(), 74);
        assert_eq!(payload, expected);
    }

    #[test]
    fn red_error_with_text_color_extra_is_seventy_five_bytes() {
        let message = "Impossible d'interagir avec ce bloc!";
        assert_eq!(message.len(), 36);
        let component = Component {
            content: Content::Text(String::new()),
            style: Style {
                color: Some("red".to_owned()),
                ..Style::default()
            },
            extra: vec![Component::text(message)],
        };
        let mut expected = Vec::new();
        expected.push(0x0A);
        expected.extend_from_slice(&[0x08, 0x00, 0x04, b't', b'e', b'x', b't', 0x00, 0x00]);
        expected.extend_from_slice(&[
            0x08, 0x00, 0x05, b'c', b'o', b'l', b'o', b'r', 0x00, 0x03, b'r', b'e', b'd',
        ]);
        expected.extend_from_slice(&[0x09, 0x00, 0x05, b'e', b'x', b't', b'r', b'a']);
        expected.extend_from_slice(&[0x08, 0x00, 0x00, 0x00, 0x01]);
        expected.push(0x00);
        expected.push(0x24);
        expected.extend_from_slice(message.as_bytes());
        expected.push(0x00);

        let payload = nbt(&component);
        assert_eq!(payload.len(), 75);
        assert_eq!(payload, expected);
    }

    #[test]
    fn heterogeneous_with_list_is_eighty_seven_bytes() {
        let string_arg = "Le temps du monde actuel :";
        assert_eq!(string_arg.len(), 26);
        let component = Component::translate("commands.time.query")
            .with(Arg::Str(string_arg.to_owned()))
            .with(Arg::Int(1000));

        let mut expected = Vec::new();
        expected.push(0x0A);
        expected.extend_from_slice(&[0x08, 0x00, 0x09]);
        expected.extend_from_slice(b"translate");
        expected.push(0x00);
        expected.push(0x13);
        expected.extend_from_slice(b"commands.time.query");
        expected.extend_from_slice(&[0x09, 0x00, 0x04, b'w', b'i', b't', b'h']);
        expected.push(0x0A);
        expected.extend_from_slice(&[0x00, 0x00, 0x00, 0x02]);
        expected.extend_from_slice(&[0x08, 0x00, 0x00]);
        expected.push(0x00);
        expected.push(0x1A);
        expected.extend_from_slice(string_arg.as_bytes());
        expected.push(0x00);
        expected.extend_from_slice(&[0x03, 0x00, 0x00]);
        expected.extend_from_slice(&1000i32.to_be_bytes());
        expected.push(0x00);
        expected.push(0x00);

        let payload = nbt(&component);
        assert_eq!(payload.len(), 87);
        assert_eq!(payload, expected);
    }

    #[test]
    fn weather_body_is_forty_two_bytes() {
        let key = "commands.weather.set.rain";
        assert_eq!(key.len(), 25);
        let component = Component::translate(key);
        let nbt_payload = nbt(&component);
        assert_eq!(nbt_payload.len(), 41);

        let body = system_chat_payload(&component, false);
        assert_eq!(body.len(), 42);
        assert_eq!(body[41], 0x00);
        assert_eq!(&body[..41], nbt_payload.as_slice());
    }

    #[test]
    fn mutf8_encodes_nul_and_non_ascii() {
        let component = Component::text("a\u{0}\u{e9}");
        assert_eq!(
            nbt(&component),
            vec![0x08, 0x00, 0x05, 0x61, 0xC0, 0x80, 0xC3, 0xA9]
        );
    }

    #[test]
    fn player_display_name_is_rich_with_insertion_click_and_hover() {
        let name = player_display_name("Alice", 0x0102_0304_0506_0708_090A_0B0C_0D0E_0F10);
        assert_eq!(name.content, Content::Text("Alice".to_owned()));
        assert_eq!(name.style.insertion.as_deref(), Some("Alice"));
        let click = name.style.click.as_ref().expect("clic présent");
        assert_eq!(click.action, "suggest_command");
        assert_eq!(click.value, "/tell Alice ");
        match name.style.hover.as_ref().expect("survol présent") {
            HoverEvent::ShowEntity {
                id,
                uuid,
                name: display,
            } => {
                assert_eq!(id, "minecraft:player");
                assert_eq!(*uuid, 0x0102_0304_0506_0708_090A_0B0C_0D0E_0F10);
                assert_eq!(**display, Component::text("Alice"));
            }
            other => panic!("survol inattendu: {other:?}"),
        }

        let mut buf = Vec::new();
        write_nbt(&mut buf, &name);
        assert_eq!(buf[0], TAG_COMPOUND);
    }

    #[test]
    fn uuid_serialises_as_four_big_endian_ints() {
        let mut buf = Vec::new();
        write_uuid_int_array(&mut buf, 0x0000_0001_0000_0002_0000_0003_0000_0004);
        assert_eq!(
            buf,
            vec![
                0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00,
                0x00, 0x03, 0x00, 0x00, 0x00, 0x04,
            ]
        );
    }
}
