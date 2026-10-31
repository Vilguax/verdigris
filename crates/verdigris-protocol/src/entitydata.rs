use crate::varint::write_var_int;

pub const ENTITY_DATA_TERMINATOR: u8 = 0xFF;

pub const SERIALIZER_BYTE: u8 = 0;
pub const SERIALIZER_INT: u8 = 1;
pub const SERIALIZER_FLOAT: u8 = 3;
pub const SERIALIZER_BOOLEAN: u8 = 8;
pub const SERIALIZER_POSE: u8 = 20;
pub const SERIALIZER_HUMANOID_ARM: u8 = 42;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Pose {
    Standing = 0,
    Crouching = 5,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum HumanoidArm {
    Left = 0,
    Right = 1,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EntityDataValue {
    Byte(i8),
    VarInt(i32),
    Float(f32),
    Bool(bool),
    Pose(Pose),
    HumanoidArm(HumanoidArm),
}

impl EntityDataValue {
    pub fn serializer_id(&self) -> u8 {
        match self {
            EntityDataValue::Byte(_) => SERIALIZER_BYTE,
            EntityDataValue::VarInt(_) => SERIALIZER_INT,
            EntityDataValue::Float(_) => SERIALIZER_FLOAT,
            EntityDataValue::Bool(_) => SERIALIZER_BOOLEAN,
            EntityDataValue::Pose(_) => SERIALIZER_POSE,
            EntityDataValue::HumanoidArm(_) => SERIALIZER_HUMANOID_ARM,
        }
    }

    pub fn write_value(&self, buf: &mut Vec<u8>) {
        match self {
            EntityDataValue::Byte(value) => buf.push(*value as u8),
            EntityDataValue::VarInt(value) => write_var_int(*value, buf),
            EntityDataValue::Float(value) => buf.extend_from_slice(&value.to_be_bytes()),
            EntityDataValue::Bool(value) => buf.push(u8::from(*value)),
            EntityDataValue::Pose(pose) => write_var_int(*pose as i32, buf),
            EntityDataValue::HumanoidArm(arm) => write_var_int(*arm as i32, buf),
        }
    }
}

pub fn append_entity_data_entries(buf: &mut Vec<u8>, entries: &[(u8, EntityDataValue)]) {
    for (index, value) in entries {
        buf.push(*index);
        buf.push(value.serializer_id());
        value.write_value(buf);
    }
}

pub fn write_set_entity_data(buf: &mut Vec<u8>, entity_id: i32, entries: &[(u8, EntityDataValue)]) {
    write_var_int(entity_id, buf);
    append_entity_data_entries(buf, entries);
    buf.push(ENTITY_DATA_TERMINATOR);
}

pub fn set_entity_data_payload(entity_id: i32, entries: &[(u8, EntityDataValue)]) -> Vec<u8> {
    let mut payload = Vec::new();
    write_set_entity_data(&mut payload, entity_id, entries);
    payload
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value_bytes(value: EntityDataValue) -> Vec<u8> {
        let mut buf = Vec::new();
        value.write_value(&mut buf);
        buf
    }

    #[test]
    fn serializer_ids_match_report() {
        assert_eq!(EntityDataValue::Byte(0).serializer_id(), 0);
        assert_eq!(EntityDataValue::VarInt(0).serializer_id(), 1);
        assert_eq!(EntityDataValue::Float(0.0).serializer_id(), 3);
        assert_eq!(EntityDataValue::Bool(false).serializer_id(), 8);
        assert_eq!(EntityDataValue::Pose(Pose::Standing).serializer_id(), 20);
        assert_eq!(
            EntityDataValue::HumanoidArm(HumanoidArm::Left).serializer_id(),
            42
        );
    }

    #[test]
    fn byte_value_is_single_octet() {
        assert_eq!(value_bytes(EntityDataValue::Byte(0x7F)), vec![0x7F]);
        assert_eq!(value_bytes(EntityDataValue::Byte(0x02)), vec![0x02]);
        assert_eq!(value_bytes(EntityDataValue::Byte(-1)), vec![0xFF]);
    }

    #[test]
    fn var_int_value_round_trips() {
        assert_eq!(value_bytes(EntityDataValue::VarInt(300)), vec![0xAC, 0x02]);
    }

    #[test]
    fn float_value_is_big_endian() {
        assert_eq!(
            value_bytes(EntityDataValue::Float(20.0)),
            vec![0x41, 0xA0, 0x00, 0x00]
        );
    }

    #[test]
    fn bool_value_is_single_octet() {
        assert_eq!(value_bytes(EntityDataValue::Bool(true)), vec![0x01]);
        assert_eq!(value_bytes(EntityDataValue::Bool(false)), vec![0x00]);
    }

    #[test]
    fn pose_value_is_var_int_of_ordinal() {
        assert_eq!(
            value_bytes(EntityDataValue::Pose(Pose::Crouching)),
            vec![0x05]
        );
        assert_eq!(
            value_bytes(EntityDataValue::Pose(Pose::Standing)),
            vec![0x00]
        );
    }

    #[test]
    fn humanoid_arm_value_is_var_int_of_ordinal() {
        assert_eq!(
            value_bytes(EntityDataValue::HumanoidArm(HumanoidArm::Left)),
            vec![0x00]
        );
        assert_eq!(
            value_bytes(EntityDataValue::HumanoidArm(HumanoidArm::Right)),
            vec![0x01]
        );
    }

    #[test]
    fn flags_and_pose_entries_with_terminator() {
        let mut buf = Vec::new();
        append_entity_data_entries(
            &mut buf,
            &[
                (0, EntityDataValue::Byte(0x02)),
                (6, EntityDataValue::Pose(Pose::Crouching)),
            ],
        );
        buf.push(ENTITY_DATA_TERMINATOR);
        assert_eq!(buf, vec![0x00, 0x00, 0x02, 0x06, 0x14, 0x05, 0xFF]);
    }

    #[test]
    fn health_entry_bytes() {
        let mut buf = Vec::new();
        append_entity_data_entries(&mut buf, &[(9, EntityDataValue::Float(20.0))]);
        assert_eq!(buf, vec![0x09, 0x03, 0x41, 0xA0, 0x00, 0x00]);
    }

    #[test]
    fn skin_entry_bytes() {
        let mut buf = Vec::new();
        append_entity_data_entries(&mut buf, &[(16, EntityDataValue::Byte(0x7F))]);
        assert_eq!(buf, vec![0x10, 0x00, 0x7F]);
    }

    #[test]
    fn left_arm_entry_bytes() {
        let mut buf = Vec::new();
        append_entity_data_entries(
            &mut buf,
            &[(15, EntityDataValue::HumanoidArm(HumanoidArm::Left))],
        );
        assert_eq!(buf, vec![0x0F, 0x2A, 0x00]);
    }

    #[test]
    fn set_entity_data_payload_prefixes_id_and_terminates() {
        let payload = set_entity_data_payload(5, &[(9, EntityDataValue::Float(20.0))]);
        assert_eq!(
            payload,
            vec![0x05, 0x09, 0x03, 0x41, 0xA0, 0x00, 0x00, 0xFF]
        );
    }
}
