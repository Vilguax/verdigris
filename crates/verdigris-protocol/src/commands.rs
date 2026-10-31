use crate::error::ProtocolError;
use crate::play::decode_bounded_string;
use crate::primitives::{write_i32, write_string};
use crate::text::{ClickEvent, Component, Content, Style};
use crate::varint::{read_var_int, write_var_int};

pub const PARSER_BOOL: i32 = 0;
pub const PARSER_FLOAT: i32 = 1;
pub const PARSER_DOUBLE: i32 = 2;
pub const PARSER_INTEGER: i32 = 3;
pub const PARSER_LONG: i32 = 4;
pub const PARSER_STRING: i32 = 5;
pub const PARSER_ENTITY: i32 = 6;
pub const PARSER_VEC3: i32 = 10;
pub const PARSER_ITEM_STACK: i32 = 14;
pub const PARSER_RESOURCE_LOCATION: i32 = 35;
pub const PARSER_GAMEMODE: i32 = 0x2a;
pub const PARSER_TIME: i32 = 0x2b;

pub const STRING_SINGLE_WORD: i32 = 0;
pub const STRING_QUOTABLE: i32 = 1;
pub const STRING_GREEDY: i32 = 2;

pub const ENTITY_SINGLE: u8 = 0x01;
pub const ENTITY_PLAYERS_ONLY: u8 = 0x02;

pub const FLAG_TYPE_MASK: u8 = 0x03;
pub const FLAG_ROOT: u8 = 0x00;
pub const FLAG_LITERAL: u8 = 0x01;
pub const FLAG_ARGUMENT: u8 = 0x02;
pub const FLAG_EXECUTABLE: u8 = 0x04;
pub const FLAG_REDIRECT: u8 = 0x08;
pub const FLAG_SUGGESTIONS: u8 = 0x10;
pub const FLAG_RESTRICTED: u8 = 0x20;

pub const ASK_SERVER: &str = "minecraft:ask_server";

pub const ENTITY_EVENT_PERMISSION_BASE: u8 = 24;
pub const MAX_PERMISSION_LEVEL: u8 = 4;
pub const RESTRICTED_LEVEL: u8 = 2;

pub const CHAT_TYPE_CHAT_HOLDER: i32 = 1;
pub const CHAT_TYPE_MSG_INCOMING_HOLDER: i32 = 3;
pub const CHAT_TYPE_MSG_OUTGOING_HOLDER: i32 = 4;
pub const CHAT_TYPE_SAY_HOLDER: i32 = 5;

pub const MAX_SUGGESTION_LEN_DEFAULT: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Parser {
    IntegerMin(i32),
    StringGreedy,
    StringWord,
    Entity { flags: u8 },
    GameMode,
    Vec3,
    ItemStack,
    TimeMin(i32),
    ResourceLocation,
}

impl Parser {
    fn id(self) -> i32 {
        match self {
            Parser::IntegerMin(_) => PARSER_INTEGER,
            Parser::StringGreedy | Parser::StringWord => PARSER_STRING,
            Parser::Entity { .. } => PARSER_ENTITY,
            Parser::GameMode => PARSER_GAMEMODE,
            Parser::Vec3 => PARSER_VEC3,
            Parser::ItemStack => PARSER_ITEM_STACK,
            Parser::TimeMin(_) => PARSER_TIME,
            Parser::ResourceLocation => PARSER_RESOURCE_LOCATION,
        }
    }

    fn write_properties(self, out: &mut Vec<u8>) {
        match self {
            Parser::IntegerMin(min) => {
                out.push(0x01);
                write_i32(min, out);
            }
            Parser::StringGreedy => write_var_int(STRING_GREEDY, out),
            Parser::StringWord => write_var_int(STRING_SINGLE_WORD, out),
            Parser::Entity { flags } => out.push(flags),
            Parser::GameMode => {}
            Parser::Vec3 => {}
            Parser::ItemStack => {}
            Parser::TimeMin(min) => write_i32(min, out),
            Parser::ResourceLocation => {}
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NodeKind {
    Root,
    Literal,
    Argument,
}

#[derive(Debug, Clone)]
struct Node {
    kind: NodeKind,
    name: &'static str,
    parser: Option<Parser>,
    executable: bool,
    redirect: Option<usize>,
    suggestions: bool,
    level: u8,
    children: Vec<usize>,
}

struct Builder {
    nodes: Vec<Node>,
}

impl Builder {
    fn new() -> Self {
        Builder {
            nodes: vec![Node {
                kind: NodeKind::Root,
                name: "",
                parser: None,
                executable: false,
                redirect: None,
                suggestions: false,
                level: 0,
                children: Vec::new(),
            }],
        }
    }

    fn literal(&mut self, parent: usize, name: &'static str, level: u8, executable: bool) -> usize {
        let index = self.nodes.len();
        self.nodes.push(Node {
            kind: NodeKind::Literal,
            name,
            parser: None,
            executable,
            redirect: None,
            suggestions: false,
            level,
            children: Vec::new(),
        });
        self.nodes[parent].children.push(index);
        index
    }

    #[allow(clippy::too_many_arguments)]
    fn argument(
        &mut self,
        parent: usize,
        name: &'static str,
        parser: Parser,
        level: u8,
        executable: bool,
        suggestions: bool,
    ) -> usize {
        let index = self.nodes.len();
        self.nodes.push(Node {
            kind: NodeKind::Argument,
            name,
            parser: Some(parser),
            executable,
            redirect: None,
            suggestions,
            level,
            children: Vec::new(),
        });
        self.nodes[parent].children.push(index);
        index
    }

    fn redirect_literal(
        &mut self,
        parent: usize,
        name: &'static str,
        level: u8,
        target: usize,
    ) -> usize {
        let index = self.literal(parent, name, level, false);
        self.nodes[index].redirect = Some(target);
        index
    }
}

fn full_tree() -> Vec<Node> {
    let mut b = Builder::new();
    let root = 0;

    let help = b.literal(root, "help", 0, true);
    b.argument(help, "command", Parser::StringGreedy, 0, true, false);

    let list = b.literal(root, "list", 0, true);
    b.literal(list, "uuids", 0, true);

    let msg = b.literal(root, "msg", 0, false);
    let msg_targets = b.argument(
        msg,
        "targets",
        Parser::Entity {
            flags: ENTITY_PLAYERS_ONLY,
        },
        0,
        false,
        false,
    );
    b.argument(msg_targets, "message", Parser::StringGreedy, 0, true, false);
    b.redirect_literal(root, "tell", 0, msg);
    b.redirect_literal(root, "w", 0, msg);

    let say = b.literal(root, "say", 2, false);
    b.argument(say, "message", Parser::StringGreedy, 2, true, false);

    let gamemode = b.literal(root, "gamemode", 2, false);
    let gamemode_mode = b.argument(gamemode, "gamemode", Parser::GameMode, 2, true, false);
    b.argument(
        gamemode_mode,
        "target",
        Parser::Entity { flags: 0 },
        2,
        true,
        false,
    );

    let give = b.literal(root, "give", 2, false);
    let give_target = b.argument(
        give,
        "target",
        Parser::Entity {
            flags: ENTITY_PLAYERS_ONLY,
        },
        2,
        false,
        false,
    );
    let give_item = b.argument(give_target, "item", Parser::ItemStack, 2, true, false);
    b.argument(give_item, "count", Parser::IntegerMin(1), 2, true, false);

    let teleport = b.literal(root, "teleport", 2, false);
    b.argument(
        teleport,
        "destination",
        Parser::Entity {
            flags: ENTITY_SINGLE,
        },
        2,
        true,
        false,
    );
    b.argument(teleport, "location", Parser::Vec3, 2, true, false);
    b.redirect_literal(root, "tp", 2, teleport);

    let time = b.literal(root, "time", 2, false);
    let time_set = b.literal(time, "set", 2, false);
    b.argument(time_set, "time", Parser::TimeMin(0), 2, true, true);
    let time_add = b.literal(time, "add", 2, false);
    b.argument(time_add, "time", Parser::TimeMin(0), 2, true, false);
    let time_query = b.literal(time, "query", 2, false);
    b.literal(time_query, "daytime", 2, true);

    let weather = b.literal(root, "weather", 2, false);
    b.literal(weather, "clear", 2, true);
    b.literal(weather, "rain", 2, true);
    b.literal(weather, "thunder", 2, true);

    let kill = b.literal(root, "kill", 2, true);
    b.argument(kill, "target", Parser::Entity { flags: 0 }, 2, true, false);

    b.nodes
}

fn flags_for(node: &Node) -> u8 {
    let mut flags = match node.kind {
        NodeKind::Root => FLAG_ROOT,
        NodeKind::Literal => FLAG_LITERAL,
        NodeKind::Argument => FLAG_ARGUMENT,
    };
    if node.executable {
        flags |= FLAG_EXECUTABLE;
    }
    if node.redirect.is_some() {
        flags |= FLAG_REDIRECT;
    }
    if node.suggestions {
        flags |= FLAG_SUGGESTIONS;
    }
    if node.level >= RESTRICTED_LEVEL {
        flags |= FLAG_RESTRICTED;
    }
    flags
}

fn visible(nodes: &[Node], index: usize, level: u8) -> bool {
    nodes[index].level <= level
}

fn bfs(nodes: &[Node], level: u8) -> (Vec<Option<usize>>, Vec<usize>) {
    let mut remap = vec![None; nodes.len()];
    let mut order = Vec::new();
    remap[0] = Some(0);
    order.push(0);
    let mut head = 0;
    while head < order.len() {
        let current = order[head];
        head += 1;
        for &child in &nodes[current].children {
            if visible(nodes, child, level) && remap[child].is_none() {
                remap[child] = Some(order.len());
                order.push(child);
            }
        }
        if let Some(target) = nodes[current].redirect
            && visible(nodes, target, level)
            && remap[target].is_none()
        {
            remap[target] = Some(order.len());
            order.push(target);
        }
    }
    (remap, order)
}

fn encode_node(node: &Node, remap: &[Option<usize>], out: &mut Vec<u8>) {
    out.push(flags_for(node));
    let children: Vec<usize> = node
        .children
        .iter()
        .filter(|&&child| visible_remapped(remap, child))
        .map(|&child| remap[child].unwrap())
        .collect();
    write_var_int(children.len() as i32, out);
    for child in children {
        write_var_int(child as i32, out);
    }
    if let Some(target) = node.redirect
        && let Some(mapped) = remap[target]
    {
        write_var_int(mapped as i32, out);
    }
    if node.kind != NodeKind::Root {
        write_string(node.name, out);
    }
    if node.kind == NodeKind::Argument
        && let Some(parser) = node.parser
    {
        write_var_int(parser.id(), out);
        parser.write_properties(out);
    }
    if node.suggestions {
        write_string(ASK_SERVER, out);
    }
}

fn visible_remapped(remap: &[Option<usize>], index: usize) -> bool {
    remap[index].is_some()
}

fn build_payload(level: u8) -> Vec<u8> {
    let nodes = full_tree();
    let (remap, order) = bfs(&nodes, level);
    let mut out = Vec::new();
    write_var_int(order.len() as i32, &mut out);
    for &index in &order {
        encode_node(&nodes[index], &remap, &mut out);
    }
    write_var_int(0, &mut out);
    out
}

fn cached_payloads() -> &'static [Vec<u8>; (MAX_PERMISSION_LEVEL as usize) + 1] {
    static PAYLOADS: std::sync::OnceLock<[Vec<u8>; (MAX_PERMISSION_LEVEL as usize) + 1]> =
        std::sync::OnceLock::new();
    PAYLOADS.get_or_init(|| std::array::from_fn(|level| build_payload(level as u8)))
}

pub fn commands_payload(level: u8) -> Vec<u8> {
    cached_payloads()[level.min(MAX_PERMISSION_LEVEL) as usize].clone()
}

pub fn root_command_names(level: u8) -> Vec<&'static str> {
    let nodes = full_tree();
    nodes[0]
        .children
        .iter()
        .filter(|&&child| visible(&nodes, child, level))
        .map(|&child| nodes[child].name)
        .collect()
}

pub fn command_visible(level: u8, name: &str) -> bool {
    let nodes = full_tree();
    nodes[0]
        .children
        .iter()
        .any(|&child| nodes[child].name == name && visible(&nodes, child, level))
}

fn node_usage(nodes: &[Node], index: usize, level: u8) -> String {
    let node = &nodes[index];
    let repr = match node.kind {
        NodeKind::Root => String::new(),
        NodeKind::Literal => node.name.to_owned(),
        NodeKind::Argument => format!("<{}>", node.name),
    };
    if let Some(target) = node.redirect
        && visible(nodes, target, level)
    {
        return format!("{repr} -> {}", nodes[target].name);
    }
    let children: Vec<usize> = node
        .children
        .iter()
        .copied()
        .filter(|&child| visible(nodes, child, level))
        .collect();
    if children.is_empty() {
        return repr;
    }
    let child_part = if children.len() == 1 {
        node_usage(nodes, children[0], level)
    } else {
        let parts: Vec<String> = children
            .iter()
            .map(|&child| match nodes[child].kind {
                NodeKind::Argument => node_usage(nodes, child, level),
                _ => nodes[child].name.to_owned(),
            })
            .collect();
        format!("({})", parts.join("|"))
    };
    if node.executable {
        format!("{repr} [{child_part}]")
    } else if repr.is_empty() {
        child_part
    } else {
        format!("{repr} {child_part}")
    }
}

pub fn command_usage(level: u8, name: &str) -> Option<String> {
    let nodes = full_tree();
    let index = nodes[0]
        .children
        .iter()
        .copied()
        .find(|&child| nodes[child].name == name && visible(&nodes, child, level))?;
    Some(format!("/{}", node_usage(&nodes, index, level)))
}

pub fn command_usages(level: u8) -> Vec<String> {
    let nodes = full_tree();
    nodes[0]
        .children
        .iter()
        .copied()
        .filter(|&child| visible(&nodes, child, level) && nodes[child].redirect.is_none())
        .map(|child| format!("/{}", node_usage(&nodes, child, level)))
        .collect()
}

pub fn entity_event_permission_payload(entity_id: i32, level: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(5);
    out.extend_from_slice(&entity_id.to_be_bytes());
    out.push(ENTITY_EVENT_PERMISSION_BASE + level.min(MAX_PERMISSION_LEVEL));
    out
}

pub fn command_suggestions_empty_payload(id: i32) -> Vec<u8> {
    let mut out = Vec::new();
    write_var_int(id, &mut out);
    write_var_int(0, &mut out);
    write_var_int(0, &mut out);
    write_var_int(0, &mut out);
    out
}

pub fn decode_command_suggestion(body: &[u8]) -> Result<(i32, String), ProtocolError> {
    let mut cursor = body;
    let id = match read_var_int(cursor)? {
        Some((value, consumed)) => {
            cursor = &cursor[consumed..];
            value
        }
        None => return Err(ProtocolError::UnexpectedEof),
    };
    let (text, consumed) = decode_bounded_string(cursor, crate::MAX_STRING_LEN)?;
    cursor = &cursor[consumed..];
    if !cursor.is_empty() {
        return Err(ProtocolError::TrailingBytes);
    }
    Ok((id, text))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgParseError {
    GameModeInvalid,
    IntegerInvalid,
    IntegerTooLow,
    EntityNotFoundPlayer,
    ItemIdInvalid,
    Vec3Invalid,
    TimeInvalid,
    Incomplete,
}

pub fn parse_gamemode(token: &str) -> Result<i32, ArgParseError> {
    match token {
        "survival" | "s" | "0" => Ok(0),
        "creative" | "c" | "1" => Ok(1),
        "adventure" | "a" | "2" => Ok(2),
        "spectator" | "sp" | "3" => Ok(3),
        _ => Err(ArgParseError::GameModeInvalid),
    }
}

pub fn parse_integer(token: &str, min: i32) -> Result<i32, ArgParseError> {
    let value: i32 = token.parse().map_err(|_| ArgParseError::IntegerInvalid)?;
    if value < min {
        return Err(ArgParseError::IntegerTooLow);
    }
    Ok(value)
}

pub fn parse_time(token: &str) -> Result<i32, ArgParseError> {
    if token.is_empty() {
        return Err(ArgParseError::TimeInvalid);
    }
    let (digits, scale) = match token.chars().last() {
        Some('d') => (&token[..token.len() - 1], 24000),
        Some('s') => (&token[..token.len() - 1], 20),
        Some('t') => (&token[..token.len() - 1], 1),
        _ => (token, 1),
    };
    let value: f32 = digits.parse().map_err(|_| ArgParseError::TimeInvalid)?;
    Ok((value * scale as f32).floor() as i32)
}

pub fn parse_resource_location(token: &str) -> Result<String, ArgParseError> {
    if token.is_empty() {
        return Err(ArgParseError::Incomplete);
    }
    let normalized = if token.contains(':') {
        token.to_owned()
    } else {
        format!("minecraft:{token}")
    };
    for ch in normalized.chars() {
        let ok = ch.is_ascii_lowercase()
            || ch.is_ascii_digit()
            || matches!(ch, '_' | '-' | '.' | '/' | ':');
        if !ok {
            return Err(ArgParseError::ItemIdInvalid);
        }
    }
    Ok(normalized)
}

pub fn parse_vec3(tokens: &[&str]) -> Result<[f64; 3], ArgParseError> {
    if tokens.len() != 3 {
        return Err(ArgParseError::Vec3Invalid);
    }
    let mut out = [0.0f64; 3];
    for (slot, token) in out.iter_mut().zip(tokens) {
        let digits = token.strip_prefix('~').unwrap_or(token);
        if digits.is_empty() {
            *slot = 0.0;
        } else {
            *slot = digits.parse().map_err(|_| ArgParseError::Vec3Invalid)?;
        }
    }
    Ok(out)
}

pub fn argument_error_component(key: &str, arg: &str) -> Component {
    red(Component::translate(key).with(crate::text::Arg::Str(arg.to_owned())))
}

fn red(inner: Component) -> Component {
    Component {
        content: Content::Text(String::new()),
        style: Style {
            color: Some("red".to_owned()),
            ..Style::default()
        },
        extra: vec![inner],
    }
}

pub fn unknown_command_components(input: &str, cursor: usize) -> Vec<Component> {
    let first = Component {
        content: Content::Text(String::new()),
        style: Style {
            color: Some("red".to_owned()),
            ..Style::default()
        },
        extra: vec![Component::translate("command.unknown.command")],
    };
    vec![first, context_component(input, cursor)]
}

pub fn context_component(input: &str, cursor: usize) -> Component {
    let chars: Vec<char> = input.chars().collect();
    let cursor = cursor.min(chars.len());
    let (ellipsis, start) = if cursor > 10 {
        (true, cursor - 10)
    } else {
        (false, 0)
    };
    let before: String = chars[start..cursor].iter().collect();
    let after: String = chars[cursor..].iter().collect();

    let mut extra = Vec::new();
    if ellipsis {
        extra.push(Component::text("…"));
    }
    if !before.is_empty() {
        extra.push(Component::text(before));
    }
    if !after.is_empty() {
        extra.push(Component {
            content: Content::Text(after),
            style: Style {
                color: Some("red".to_owned()),
                underlined: Some(true),
                ..Style::default()
            },
            extra: Vec::new(),
        });
    }
    extra.push(Component {
        content: Content::Translatable {
            key: "command.context.here".to_owned(),
            args: Vec::new(),
        },
        style: Style {
            color: Some("red".to_owned()),
            italic: Some(true),
            ..Style::default()
        },
        extra: Vec::new(),
    });

    Component {
        content: Content::Text(String::new()),
        style: Style {
            color: Some("gray".to_owned()),
            click: Some(ClickEvent {
                action: "suggest_command".to_owned(),
                value: format!("/{input}"),
            }),
            ..Style::default()
        },
        extra,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::write_nbt;

    type DecodedNode = (u8, Vec<usize>, Option<usize>, Option<String>);

    fn decode_tree(payload: &[u8]) -> (Vec<DecodedNode>, usize) {
        let mut cursor = payload;
        let (count, c) = read_var_int(cursor).unwrap().unwrap();
        cursor = &cursor[c..];
        let mut nodes = Vec::new();
        for _ in 0..count {
            let flags = cursor[0];
            cursor = &cursor[1..];
            let (nchildren, c) = read_var_int(cursor).unwrap().unwrap();
            cursor = &cursor[c..];
            let mut children = Vec::new();
            for _ in 0..nchildren {
                let (child, c) = read_var_int(cursor).unwrap().unwrap();
                cursor = &cursor[c..];
                children.push(child as usize);
            }
            let redirect = if flags & FLAG_REDIRECT != 0 {
                let (target, c) = read_var_int(cursor).unwrap().unwrap();
                cursor = &cursor[c..];
                Some(target as usize)
            } else {
                None
            };
            let name = if flags & FLAG_TYPE_MASK != FLAG_ROOT {
                let (len, c) = read_var_int(cursor).unwrap().unwrap();
                cursor = &cursor[c..];
                let text = core::str::from_utf8(&cursor[..len as usize])
                    .unwrap()
                    .to_owned();
                cursor = &cursor[len as usize..];
                Some(text)
            } else {
                None
            };
            if flags & FLAG_TYPE_MASK == FLAG_ARGUMENT {
                let (parser, c) = read_var_int(cursor).unwrap().unwrap();
                cursor = &cursor[c..];
                match parser {
                    PARSER_INTEGER => {
                        let pflags = cursor[0];
                        cursor = &cursor[1..];
                        if pflags & 0x01 != 0 {
                            cursor = &cursor[4..];
                        }
                        if pflags & 0x02 != 0 {
                            cursor = &cursor[4..];
                        }
                    }
                    PARSER_STRING => {
                        let (_, c) = read_var_int(cursor).unwrap().unwrap();
                        cursor = &cursor[c..];
                    }
                    PARSER_ENTITY => cursor = &cursor[1..],
                    PARSER_TIME => cursor = &cursor[4..],
                    _ => {}
                }
            }
            if flags & FLAG_SUGGESTIONS != 0 {
                let (len, c) = read_var_int(cursor).unwrap().unwrap();
                cursor = &cursor[c..];
                cursor = &cursor[len as usize..];
            }
            nodes.push((flags, children, redirect, name));
        }
        let (root, c) = read_var_int(cursor).unwrap().unwrap();
        cursor = &cursor[c..];
        assert!(cursor.is_empty());
        (nodes, root as usize)
    }

    #[test]
    fn full_tree_builds_without_panic() {
        let nodes = full_tree();
        assert!(nodes.len() > 10);
        for node in &nodes {
            for &child in &node.children {
                assert!(child < nodes.len());
            }
            if let Some(target) = node.redirect {
                assert!(target < nodes.len());
            }
        }
    }

    #[test]
    fn level_zero_root_children_are_exactly_social_commands() {
        assert_eq!(
            root_command_names(0),
            vec!["help", "list", "msg", "tell", "w"]
        );
    }

    #[test]
    fn level_four_root_children_include_operator_commands() {
        let names = root_command_names(4);
        for expected in [
            "help", "list", "msg", "tell", "w", "say", "gamemode", "give", "teleport", "tp",
            "time", "weather", "kill",
        ] {
            assert!(names.contains(&expected), "{expected} manquant");
        }
    }

    #[test]
    fn payload_decodes_and_root_index_is_zero() {
        let payload = commands_payload(0);
        let (nodes, root) = decode_tree(&payload);
        assert_eq!(root, 0);
        assert_eq!(nodes[0].0 & FLAG_TYPE_MASK, FLAG_ROOT);
        let literal_names: Vec<String> = nodes[0]
            .1
            .iter()
            .map(|&i| nodes[i].3.clone().unwrap())
            .collect();
        assert_eq!(literal_names, vec!["help", "list", "msg", "tell", "w"]);
    }

    #[test]
    fn tp_redirects_to_teleport_index() {
        let payload = commands_payload(4);
        let (nodes, _) = decode_tree(&payload);
        let tp = nodes
            .iter()
            .position(|n| n.3.as_deref() == Some("tp"))
            .unwrap();
        let teleport = nodes
            .iter()
            .position(|n| n.3.as_deref() == Some("teleport"))
            .unwrap();
        assert_eq!(nodes[tp].2, Some(teleport));
    }

    #[test]
    fn give_count_node_exact_bytes() {
        let mut out = Vec::new();
        let node = Node {
            kind: NodeKind::Argument,
            name: "count",
            parser: Some(Parser::IntegerMin(1)),
            executable: true,
            redirect: None,
            suggestions: false,
            level: 2,
            children: Vec::new(),
        };
        let remap = vec![Some(0usize)];
        let node = Node { level: 0, ..node };
        encode_node(&node, &remap, &mut out);
        assert_eq!(
            out,
            vec![
                0x06, 0x00, 0x05, 0x63, 0x6f, 0x75, 0x6e, 0x74, 0x03, 0x01, 0x00, 0x00, 0x00, 0x01
            ]
        );
    }

    #[test]
    fn integer_properties_min_one() {
        let mut out = Vec::new();
        Parser::IntegerMin(1).write_properties(&mut out);
        assert_eq!(out, vec![0x01, 0x00, 0x00, 0x00, 0x01]);
    }

    #[test]
    fn time_properties_min_zero_with_parser_id() {
        let mut out = Vec::new();
        write_var_int(Parser::TimeMin(0).id(), &mut out);
        Parser::TimeMin(0).write_properties(&mut out);
        assert_eq!(out, vec![0x2b, 0x00, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn entity_properties_flags() {
        let mut out = Vec::new();
        Parser::Entity {
            flags: ENTITY_PLAYERS_ONLY,
        }
        .write_properties(&mut out);
        assert_eq!(out, vec![0x02]);
    }

    #[test]
    fn gamemode_node_carries_restricted_and_absent_for_level_zero() {
        let op = commands_payload(4);
        let (op_nodes, _) = decode_tree(&op);
        let gamemode = op_nodes
            .iter()
            .find(|n| n.3.as_deref() == Some("gamemode"))
            .unwrap();
        assert_ne!(gamemode.0 & FLAG_RESTRICTED, 0);

        let social = commands_payload(0);
        let (social_nodes, _) = decode_tree(&social);
        assert!(
            social_nodes
                .iter()
                .all(|n| n.3.as_deref() != Some("gamemode"))
        );
    }

    #[test]
    fn time_value_node_has_ask_server_suggestion() {
        let payload = commands_payload(4);
        let (nodes, _) = decode_tree(&payload);
        let suggesting = nodes
            .iter()
            .find(|n| n.0 & FLAG_SUGGESTIONS != 0)
            .expect("un noeud porte ask_server");
        assert_eq!(suggesting.3.as_deref(), Some("time"));
    }

    #[test]
    fn entity_event_permission_bytes() {
        assert_eq!(
            entity_event_permission_payload(7, 0),
            vec![0x00, 0x00, 0x00, 0x07, 24]
        );
        assert_eq!(
            entity_event_permission_payload(7, 4),
            vec![0x00, 0x00, 0x00, 0x07, 28]
        );
    }

    #[test]
    fn suggestions_empty_payload_varint_id() {
        assert_eq!(
            command_suggestions_empty_payload(1),
            vec![0x01, 0x00, 0x00, 0x00]
        );
        assert_eq!(
            command_suggestions_empty_payload(300),
            vec![0xac, 0x02, 0x00, 0x00, 0x00]
        );
    }

    #[test]
    fn decode_command_suggestion_reads_id_and_text() {
        let mut body = Vec::new();
        write_var_int(5, &mut body);
        write_string("gamemode ", &mut body);
        assert_eq!(
            decode_command_suggestion(&body).unwrap(),
            (5, "gamemode ".to_owned())
        );
    }

    #[test]
    fn parse_gamemode_rejects_unknown() {
        assert_eq!(parse_gamemode("creative"), Ok(1));
        assert_eq!(parse_gamemode("foo"), Err(ArgParseError::GameModeInvalid));
    }

    #[test]
    fn parse_time_suffixes() {
        assert_eq!(parse_time("1d"), Ok(24000));
        assert_eq!(parse_time("20s"), Ok(400));
        assert_eq!(parse_time("5t"), Ok(5));
        assert_eq!(parse_time("10"), Ok(10));
        assert_eq!(parse_time("x"), Err(ArgParseError::TimeInvalid));
    }

    #[test]
    fn smart_usage_gamemode() {
        assert_eq!(
            command_usage(4, "gamemode").as_deref(),
            Some("/gamemode <gamemode> [<target>]")
        );
    }

    #[test]
    fn argument_error_is_red_with_translate() {
        let component = argument_error_component("argument.gamemode.invalid", "foo");
        assert_eq!(component.style.color.as_deref(), Some("red"));
        let mut buf = Vec::new();
        write_nbt(&mut buf, &component);
        assert!(!buf.is_empty());
    }

    #[test]
    fn unknown_command_yields_two_components() {
        let components = unknown_command_components("frobnicate", 0);
        assert_eq!(components.len(), 2);
        assert_eq!(components[0].style.color.as_deref(), Some("red"));
        match &components[0].extra[0].content {
            Content::Translatable { key, .. } => assert_eq!(key, "command.unknown.command"),
            other => panic!("inattendu {other:?}"),
        }
        assert_eq!(components[1].style.color.as_deref(), Some("gray"));
    }

    #[test]
    fn context_shows_ellipsis_past_ten_chars() {
        let component = context_component("abcdefghijklmnop", 16);
        match &component.extra[0].content {
            Content::Text(text) => assert_eq!(text, "…"),
            other => panic!("inattendu {other:?}"),
        }
    }
}
