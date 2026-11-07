use crate::error::ProtocolError;
use crate::play::{
    ItemStack, ensure_consumed, take_bool, take_i16, take_u8, take_var_int,
    write_item_stack_no_components,
};
use crate::primitives::read_i32;
use crate::text::{Component, write_nbt};
use crate::varint::write_var_int;

pub const MAX_CHANGED_SLOTS: usize = 128;
pub const MAX_HASHED_COMPONENTS: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClickType {
    Pickup,
    QuickMove,
    Swap,
    Clone,
    Throw,
    QuickCraft,
    PickupAll,
}

impl ClickType {
    pub fn from_raw(raw: i32) -> ClickType {
        match raw {
            1 => ClickType::QuickMove,
            2 => ClickType::Swap,
            3 => ClickType::Clone,
            4 => ClickType::Throw,
            5 => ClickType::QuickCraft,
            6 => ClickType::PickupAll,
            _ => ClickType::Pickup,
        }
    }

    pub fn id(self) -> i32 {
        match self {
            ClickType::Pickup => 0,
            ClickType::QuickMove => 1,
            ClickType::Swap => 2,
            ClickType::Clone => 3,
            ClickType::Throw => 4,
            ClickType::QuickCraft => 5,
            ClickType::PickupAll => 6,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HashedStack {
    Empty,
    Item {
        item: i32,
        count: i32,
        added: Vec<(i32, i32)>,
        removed: Vec<i32>,
    },
}

impl HashedStack {
    pub fn matches(&self, stack: ItemStack) -> bool {
        let normalized = match stack {
            ItemStack::Item { count, item } if count > 0 => Some((item, count)),
            _ => None,
        };
        match self {
            HashedStack::Empty => normalized.is_none(),
            HashedStack::Item {
                item,
                count,
                added,
                removed,
            } => {
                if !added.is_empty() || !removed.is_empty() {
                    return false;
                }
                match normalized {
                    Some((i, c)) => *item == i && *count == c,
                    None => false,
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerClick {
    pub container_id: i32,
    pub state_id: i32,
    pub slot: i16,
    pub button: i8,
    pub mode: ClickType,
    pub changed_slots: Vec<(i16, HashedStack)>,
    pub carried: HashedStack,
}

fn take_i32(cursor: &mut &[u8]) -> Result<i32, ProtocolError> {
    match read_i32(cursor)? {
        Some((value, consumed)) => {
            *cursor = &cursor[consumed..];
            Ok(value)
        }
        None => Err(ProtocolError::UnexpectedEof),
    }
}

fn check_bound(len: i32, max: usize) -> Result<(), ProtocolError> {
    if len < 0 {
        return Err(ProtocolError::PrefixedArrayLengthNegative);
    }
    if len as i64 > max as i64 {
        return Err(ProtocolError::PrefixedArrayTooLong);
    }
    Ok(())
}

pub fn decode_hashed_stack(cursor: &mut &[u8]) -> Result<HashedStack, ProtocolError> {
    if !take_bool(cursor)? {
        return Ok(HashedStack::Empty);
    }
    let item = take_var_int(cursor)?;
    let count = take_var_int(cursor)?;

    let added_len = take_var_int(cursor)?;
    check_bound(added_len, MAX_HASHED_COMPONENTS)?;
    let mut added = Vec::with_capacity(added_len as usize);
    for _ in 0..added_len {
        let kind = take_var_int(cursor)?;
        let hash = take_i32(cursor)?;
        added.push((kind, hash));
    }

    let removed_len = take_var_int(cursor)?;
    check_bound(removed_len, MAX_HASHED_COMPONENTS)?;
    let mut removed = Vec::with_capacity(removed_len as usize);
    for _ in 0..removed_len {
        removed.push(take_var_int(cursor)?);
    }

    Ok(HashedStack::Item {
        item,
        count,
        added,
        removed,
    })
}

pub fn decode_container_click(body: &[u8]) -> Result<ContainerClick, ProtocolError> {
    let mut cursor = body;
    let container_id = take_var_int(&mut cursor)?;
    let state_id = take_var_int(&mut cursor)?;
    let slot = take_i16(&mut cursor)?;
    let button = take_u8(&mut cursor)? as i8;
    let mode = ClickType::from_raw(take_var_int(&mut cursor)?);

    let changed_len = take_var_int(&mut cursor)?;
    check_bound(changed_len, MAX_CHANGED_SLOTS)?;
    let mut changed_slots = Vec::with_capacity(changed_len as usize);
    for _ in 0..changed_len {
        let slot = take_i16(&mut cursor)?;
        let stack = decode_hashed_stack(&mut cursor)?;
        changed_slots.push((slot, stack));
    }

    let carried = decode_hashed_stack(&mut cursor)?;
    ensure_consumed(cursor)?;
    Ok(ContainerClick {
        container_id,
        state_id,
        slot,
        button,
        mode,
        changed_slots,
        carried,
    })
}

pub fn decode_container_close(body: &[u8]) -> Result<i32, ProtocolError> {
    let mut cursor = body;
    let container_id = take_var_int(&mut cursor)?;
    ensure_consumed(cursor)?;
    Ok(container_id)
}

pub fn container_set_content_payload(
    window_id: i32,
    state_id: i32,
    items: &[ItemStack],
    carried: ItemStack,
) -> Vec<u8> {
    let mut payload = Vec::new();
    write_var_int(window_id, &mut payload);
    write_var_int(state_id, &mut payload);
    write_var_int(items.len() as i32, &mut payload);
    for &item in items {
        write_item_stack_no_components(item, &mut payload);
    }
    write_item_stack_no_components(carried, &mut payload);
    payload
}

pub fn set_cursor_item_payload(stack: ItemStack) -> Vec<u8> {
    let mut payload = Vec::new();
    write_item_stack_no_components(stack, &mut payload);
    payload
}

pub fn set_player_inventory_payload(slot: i32, stack: ItemStack) -> Vec<u8> {
    let mut payload = Vec::new();
    write_var_int(slot, &mut payload);
    write_item_stack_no_components(stack, &mut payload);
    payload
}

pub fn container_close_payload(window_id: i32) -> Vec<u8> {
    let mut payload = Vec::new();
    write_var_int(window_id, &mut payload);
    payload
}

pub fn open_screen_payload(window_id: i32, menu_type: i32, title: &Component) -> Vec<u8> {
    let mut payload = Vec::new();
    write_var_int(window_id, &mut payload);
    write_var_int(menu_type, &mut payload);
    write_nbt(&mut payload, title);
    payload
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hashed_item(item: i32, count: i32) -> HashedStack {
        HashedStack::Item {
            item,
            count,
            added: Vec::new(),
            removed: Vec::new(),
        }
    }

    #[test]
    fn click_type_round_trip_and_out_of_range() {
        for raw in 0..=6 {
            assert_eq!(ClickType::from_raw(raw).id(), raw);
        }
        assert_eq!(ClickType::from_raw(7), ClickType::Pickup);
        assert_eq!(ClickType::from_raw(-1), ClickType::Pickup);
        assert_eq!(ClickType::from_raw(i32::MAX), ClickType::Pickup);
    }

    #[test]
    fn decode_container_click_full_vector() {
        let body = [
            0x01, 0x02, 0x00, 0x03, 0x01, 0x00, 0x02, 0x00, 0x00, 0x01, 0x01, 0x01, 0x00, 0x00,
            0x00, 0x01, 0x00, 0x00,
        ];
        let click = decode_container_click(&body).unwrap();
        let expected = ContainerClick {
            container_id: 1,
            state_id: 2,
            slot: 3,
            button: 1,
            mode: ClickType::Pickup,
            changed_slots: vec![(0, hashed_item(1, 1)), (1, HashedStack::Empty)],
            carried: HashedStack::Empty,
        };
        assert_eq!(click, expected);
    }

    #[test]
    fn decode_container_click_preserves_order_and_duplicates() {
        let body = [
            0x07, 0x09, 0xFF, 0xFB, 0xFF, 0x01, 0x03, 0x00, 0x05, 0x00, 0x00, 0x05, 0x00, 0x00,
            0x02, 0x01, 0x0A, 0x40, 0x01, 0x07, 0x11, 0x22, 0x33, 0x44, 0x01, 0x63, 0x00,
        ];
        let click = decode_container_click(&body).unwrap();
        assert_eq!(click.container_id, 7);
        assert_eq!(click.state_id, 9);
        assert_eq!(click.slot, -5);
        assert_eq!(click.button, -1);
        assert_eq!(click.mode, ClickType::QuickMove);
        assert_eq!(click.changed_slots.len(), 3);
        assert_eq!(click.changed_slots[0], (5, HashedStack::Empty));
        assert_eq!(click.changed_slots[1], (5, HashedStack::Empty));
        let (slot2, stack2) = &click.changed_slots[2];
        assert_eq!(*slot2, 2);
        let expected2 = HashedStack::Item {
            item: 10,
            count: 64,
            added: vec![(7, 0x1122_3344)],
            removed: vec![99],
        };
        assert_eq!(*stack2, expected2);
        assert_eq!(click.carried, HashedStack::Empty);
    }

    #[test]
    fn decode_container_click_mode_out_of_range_becomes_pickup() {
        let body = [0x00, 0x00, 0x00, 0x00, 0x00, 0x07, 0x00, 0x00];
        let click = decode_container_click(&body).unwrap();
        assert_eq!(click.mode, ClickType::Pickup);
    }

    #[test]
    fn decode_container_click_changed_slots_bounds() {
        let over = [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x81, 0x01];
        let over = decode_container_click(&over);
        assert_eq!(over, Err(ProtocolError::PrefixedArrayTooLong));
        let at_limit = [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80, 0x01];
        let at_limit = decode_container_click(&at_limit);
        assert_eq!(at_limit, Err(ProtocolError::UnexpectedEof));
        let negative = [
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0x0F,
        ];
        let negative = decode_container_click(&negative);
        assert_eq!(negative, Err(ProtocolError::PrefixedArrayLengthNegative));
    }

    #[test]
    fn decode_hashed_stack_component_bounds() {
        let over = [0x01, 0x01, 0x01, 0x81, 0x02];
        let over = decode_hashed_stack(&mut &over[..]);
        assert_eq!(over, Err(ProtocolError::PrefixedArrayTooLong));
        let at_limit = [0x01, 0x01, 0x01, 0x80, 0x02];
        let at_limit = decode_hashed_stack(&mut &at_limit[..]);
        assert_eq!(at_limit, Err(ProtocolError::UnexpectedEof));
        let negative = [0x01, 0x01, 0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0x0F];
        let negative = decode_hashed_stack(&mut &negative[..]);
        assert_eq!(negative, Err(ProtocolError::PrefixedArrayLengthNegative));
        let removed_over = [0x01, 0x01, 0x01, 0x00, 0x81, 0x02];
        let removed_over = decode_hashed_stack(&mut &removed_over[..]);
        assert_eq!(removed_over, Err(ProtocolError::PrefixedArrayTooLong));
    }

    #[test]
    fn decode_hashed_stack_empty_reads_single_byte() {
        let buf = [0x00, 0xAA];
        let mut cursor = &buf[..];
        assert_eq!(
            decode_hashed_stack(&mut cursor).unwrap(),
            HashedStack::Empty
        );
        assert_eq!(cursor, &[0xAA]);
    }

    #[test]
    fn decode_container_click_truncated_is_eof() {
        let body = [0x01, 0x02, 0x00];
        let result = decode_container_click(&body);
        assert_eq!(result, Err(ProtocolError::UnexpectedEof));
    }

    #[test]
    fn decode_container_click_trailing_is_error() {
        let body = [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF];
        let result = decode_container_click(&body);
        assert_eq!(result, Err(ProtocolError::TrailingBytes));
    }

    #[test]
    fn decode_hashed_stack_truncated_is_eof() {
        let buf = [0x01, 0x01];
        let result = decode_hashed_stack(&mut &buf[..]);
        assert_eq!(result, Err(ProtocolError::UnexpectedEof));
    }

    #[test]
    fn decode_container_close_round_trip() {
        assert_eq!(decode_container_close(&[0x2A]).unwrap(), 42);
        let trailing = decode_container_close(&[0x2A, 0x00]);
        assert_eq!(trailing, Err(ProtocolError::TrailingBytes));
        let empty = decode_container_close(&[]);
        assert_eq!(empty, Err(ProtocolError::UnexpectedEof));
    }

    #[test]
    fn matches_empty_against_stacks() {
        assert!(HashedStack::Empty.matches(ItemStack::Empty));
        assert!(HashedStack::Empty.matches(ItemStack::Item { count: 0, item: 1 }));
        assert!(HashedStack::Empty.matches(ItemStack::Item { count: -3, item: 1 }));
        assert!(!HashedStack::Empty.matches(ItemStack::Item { count: 1, item: 1 }));
    }

    #[test]
    fn matches_item_rules() {
        let stack = hashed_item(1, 1);
        assert!(stack.matches(ItemStack::Item { count: 1, item: 1 }));
        assert!(!stack.matches(ItemStack::Item { count: 2, item: 1 }));
        assert!(!stack.matches(ItemStack::Item { count: 1, item: 2 }));
        assert!(!stack.matches(ItemStack::Empty));
        assert!(!stack.matches(ItemStack::Item { count: 0, item: 1 }));

        let with_components = HashedStack::Item {
            item: 1,
            count: 1,
            added: vec![(3, 0x1234)],
            removed: Vec::new(),
        };
        assert!(!with_components.matches(ItemStack::Item { count: 1, item: 1 }));
    }

    #[test]
    fn encode_container_close_byte_exact() {
        assert_eq!(container_close_payload(5), vec![0x05]);
    }

    #[test]
    fn encode_set_cursor_item_byte_exact() {
        assert_eq!(set_cursor_item_payload(ItemStack::Empty), vec![0x00]);
        assert_eq!(
            set_cursor_item_payload(ItemStack::Item { count: 1, item: 1 }),
            vec![0x01, 0x01, 0x00, 0x00]
        );
    }

    #[test]
    fn encode_set_player_inventory_byte_exact() {
        assert_eq!(
            set_player_inventory_payload(9, ItemStack::Item { count: 64, item: 1 }),
            vec![0x09, 0x40, 0x01, 0x00, 0x00]
        );
    }

    #[test]
    fn encode_container_set_content_byte_exact() {
        let items = [ItemStack::Empty, ItemStack::Item { count: 1, item: 1 }];
        let carried = ItemStack::Item { count: 5, item: 3 };
        let payload = container_set_content_payload(1, 2, &items, carried);
        let mut expected = vec![0x01, 0x02, 0x02];
        expected.extend_from_slice(&[0x00, 0x01, 0x01, 0x00, 0x00, 0x05, 0x03, 0x00, 0x00]);
        assert_eq!(payload, expected);
    }

    #[test]
    fn encode_open_screen_byte_exact() {
        let title = Component::translate("container.chest");
        let payload = open_screen_payload(1, 2, &title);
        let mut expected = vec![0x01, 0x02];
        expected.extend_from_slice(&[0x0a, 0x08, 0x00, 0x09]);
        expected.extend_from_slice(b"translate");
        expected.extend_from_slice(&[0x00, 0x0f]);
        expected.extend_from_slice(b"container.chest");
        expected.push(0x00);
        assert_eq!(payload, expected);
    }
}
