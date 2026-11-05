use verdigris_protocol::{
    Arg, ArgParseError, Component, item_component, item_id_of_name, parse_integer,
    player_display_name,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedItem {
    pub name: String,
    pub id: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemError {
    Syntax,
    Unknown(String),
}

pub fn parse_item_token(token: &str) -> Result<ParsedItem, ItemError> {
    if token.is_empty() || token.contains('[') || token.contains('{') || token.contains('#') {
        return Err(ItemError::Syntax);
    }
    let name = if token.contains(':') {
        token.to_owned()
    } else {
        format!("minecraft:{token}")
    };
    match item_id_of_name(&name) {
        Some(id) => Ok(ParsedItem { name, id }),
        None => Err(ItemError::Unknown(token.to_owned())),
    }
}

pub fn parse_count(token: Option<&str>) -> Result<i32, ArgParseError> {
    match token {
        None => Ok(1),
        Some(value) => parse_integer(value, 1),
    }
}

pub fn too_many_items_component(cap: i32, item_name: &str) -> Component {
    Component::translate("commands.give.failed.toomanyitems")
        .color("red")
        .with(Arg::Int(cap))
        .with(Arg::Comp(Box::new(item_component(item_name, 1))))
}

pub fn success_single(
    count: i32,
    item_name: &str,
    target_name: &str,
    target_uuid: u128,
) -> Component {
    Component::translate("commands.give.success.single")
        .with(Arg::Int(count))
        .with(Arg::Comp(Box::new(item_component(item_name, count))))
        .with(Arg::Comp(Box::new(player_display_name(
            target_name,
            target_uuid,
        ))))
}

pub fn success_multiple(count: i32, item_name: &str, target_count: i32) -> Component {
    Component::translate("commands.give.success.multiple")
        .with(Arg::Int(count))
        .with(Arg::Comp(Box::new(item_component(item_name, count))))
        .with(Arg::Int(target_count))
}

#[cfg(test)]
mod tests {
    use super::*;
    use verdigris_protocol::Content;

    #[test]
    fn parse_item_accepts_bare_and_namespaced() {
        let bare = parse_item_token("diamond").unwrap();
        let full = parse_item_token("minecraft:diamond").unwrap();
        assert_eq!(bare.name, "minecraft:diamond");
        assert_eq!(bare.id, full.id);
    }

    #[test]
    fn give_objet_inconnu() {
        assert_eq!(
            parse_item_token("diamondz"),
            Err(ItemError::Unknown("diamondz".to_owned()))
        );
    }

    #[test]
    fn give_crochets_rejetes() {
        assert_eq!(
            parse_item_token("diamond[damage=1]"),
            Err(ItemError::Syntax)
        );
    }

    #[test]
    fn give_tag_diese_rejete() {
        assert_eq!(parse_item_token("#planks"), Err(ItemError::Syntax));
    }

    #[test]
    fn give_count_defaut_1() {
        assert_eq!(parse_count(None), Ok(1));
        assert_eq!(parse_count(Some("5")), Ok(5));
        assert_eq!(parse_count(Some("0")), Err(ArgParseError::IntegerTooLow));
        assert_eq!(parse_count(Some("x")), Err(ArgParseError::IntegerInvalid));
    }

    fn translate_key(component: &Component) -> &str {
        match &component.content {
            Content::Translatable { key, .. } => key,
            other => panic!("attendu translatable, obtenu {other:?}"),
        }
    }

    fn args(component: &Component) -> &[Arg] {
        match &component.content {
            Content::Translatable { args, .. } => args,
            other => panic!("attendu translatable, obtenu {other:?}"),
        }
    }

    #[test]
    fn give_plafond_component() {
        let component = too_many_items_component(1600, "minecraft:ender_pearl");
        assert_eq!(
            translate_key(&component),
            "commands.give.failed.toomanyitems"
        );
        assert_eq!(component.style.color.as_deref(), Some("red"));
        assert_eq!(args(&component)[0], Arg::Int(1600));
        match &args(&component)[1] {
            Arg::Comp(item) => assert_eq!(translate_key(item), "chat.square_brackets"),
            other => panic!("objet inattendu {other:?}"),
        }
    }

    #[test]
    fn give_retour_single() {
        let component = success_single(5, "minecraft:diamond", "Alice", 42);
        assert_eq!(translate_key(&component), "commands.give.success.single");
        let args = args(&component);
        assert_eq!(args[0], Arg::Int(5));
        match &args[1] {
            Arg::Comp(item) => {
                assert_eq!(item.style.color.as_deref(), Some("white"));
                assert_eq!(translate_key(item), "chat.square_brackets");
            }
            other => panic!("objet inattendu {other:?}"),
        }
        match &args[2] {
            Arg::Comp(target) => {
                assert_eq!(target.content, Content::Text("Alice".to_owned()));
            }
            other => panic!("cible inattendue {other:?}"),
        }
    }

    #[test]
    fn give_retour_multiple() {
        let component = success_multiple(3, "minecraft:dirt", 2);
        assert_eq!(translate_key(&component), "commands.give.success.multiple");
        let args = args(&component);
        assert_eq!(args[0], Arg::Int(3));
        match &args[1] {
            Arg::Comp(item) => assert_eq!(translate_key(item), "chat.square_brackets"),
            other => panic!("objet inattendu {other:?}"),
        }
        assert_eq!(args[2], Arg::Int(2));
    }
}
