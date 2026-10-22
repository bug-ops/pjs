//! Single authoritative wire encoding for value objects that do not implement
//! `Serialize`/`Deserialize` themselves.
//!
//! Each submodule is meant to be used through `#[serde(with = "...")]`:
//! `Priority` travels as a `u8` (validated on decode), `SessionId`/`StreamId`
//! as a UUID. Stream collections are encoded by `entities::StreamMap`.

/// `Priority` as a bare `u8`; decoding rejects values that `Priority::new` refuses.
pub mod priority {
    use crate::value_objects::Priority;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    /// Serializes the priority as its `u8` value.
    pub fn serialize<S: Serializer>(priority: &Priority, serializer: S) -> Result<S::Ok, S::Error> {
        priority.value().serialize(serializer)
    }

    /// Deserializes a `u8` and validates it through `Priority::new`.
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Priority, D::Error> {
        Priority::new(u8::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// `HashMap<String, Priority>` as a map of `u8` values, validated on decode.
pub mod priority_map {
    use crate::value_objects::Priority;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::collections::HashMap;

    /// Serializes each priority as its `u8` value.
    pub fn serialize<S: Serializer>(
        map: &HashMap<String, Priority>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let raw: HashMap<&String, u8> = map.iter().map(|(k, v)| (k, v.value())).collect();
        raw.serialize(serializer)
    }

    /// Deserializes and validates every value through `Priority::new`.
    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<HashMap<String, Priority>, D::Error> {
        HashMap::<String, u8>::deserialize(deserializer)?
            .into_iter()
            .map(|(k, v)| {
                Priority::new(v)
                    .map(|p| (k, p))
                    .map_err(serde::de::Error::custom)
            })
            .collect()
    }
}

/// `SessionId` as a UUID.
pub mod session_id {
    use crate::value_objects::SessionId;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    /// Serializes the id as its UUID.
    pub fn serialize<S: Serializer>(id: &SessionId, serializer: S) -> Result<S::Ok, S::Error> {
        id.as_uuid().serialize(serializer)
    }

    /// Deserializes a UUID into a `SessionId`.
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<SessionId, D::Error> {
        uuid::Uuid::deserialize(deserializer).map(SessionId::from_uuid)
    }
}

/// `StreamId` as a UUID.
pub mod stream_id {
    use crate::value_objects::StreamId;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    /// Serializes the id as its UUID.
    pub fn serialize<S: Serializer>(id: &StreamId, serializer: S) -> Result<S::Ok, S::Error> {
        id.as_uuid().serialize(serializer)
    }

    /// Deserializes a UUID into a `StreamId`.
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<StreamId, D::Error> {
        uuid::Uuid::deserialize(deserializer).map(StreamId::from_uuid)
    }
}

/// `Option<StreamId>` as an optional UUID.
pub mod option_stream_id {
    use crate::value_objects::StreamId;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    /// Serializes the id as an optional UUID.
    pub fn serialize<S: Serializer>(
        id: &Option<StreamId>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        id.map(|i| i.as_uuid()).serialize(serializer)
    }

    /// Deserializes an optional UUID into an `Option<StreamId>`.
    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<StreamId>, D::Error> {
        Ok(Option::<uuid::Uuid>::deserialize(deserializer)?.map(StreamId::from_uuid))
    }
}

/// `Option<SessionId>` as an optional UUID.
///
/// Pair with `#[serde(default)]` so a missing field decodes to `None`.
pub mod option_session_id {
    use crate::value_objects::SessionId;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    /// Serializes the id as an optional UUID.
    pub fn serialize<S: Serializer>(
        id: &Option<SessionId>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        id.map(|i| i.as_uuid()).serialize(serializer)
    }

    /// Deserializes an optional UUID into an `Option<SessionId>`.
    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<SessionId>, D::Error> {
        Ok(Option::<uuid::Uuid>::deserialize(deserializer)?.map(SessionId::from_uuid))
    }
}

#[cfg(test)]
mod tests {
    use crate::value_objects::{Priority, SessionId, StreamId};
    use serde::{Deserialize, Serialize};
    use std::collections::HashMap;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Wire {
        #[serde(with = "super::priority")]
        priority: Priority,
        #[serde(with = "super::priority_map")]
        map: HashMap<String, Priority>,
        #[serde(with = "super::session_id")]
        session: SessionId,
        #[serde(with = "super::stream_id")]
        stream: StreamId,
        #[serde(with = "super::option_stream_id")]
        opt: Option<StreamId>,
        #[serde(with = "super::option_session_id", default)]
        opt_session: Option<SessionId>,
    }

    fn sample() -> Wire {
        Wire {
            priority: Priority::new(7).unwrap(),
            map: HashMap::from([("a".to_string(), Priority::new(3).unwrap())]),
            session: SessionId::new(),
            stream: StreamId::new(),
            opt: Some(StreamId::new()),
            opt_session: Some(SessionId::new()),
        }
    }

    #[test]
    fn wire_format_is_plain_u8_and_uuid() {
        let w = sample();
        let json = serde_json::to_value(&w).unwrap();
        assert_eq!(json["priority"], 7);
        assert_eq!(json["map"]["a"], 3);
        assert_eq!(json["session"], w.session.as_uuid().to_string());
        assert_eq!(json["stream"], w.stream.as_uuid().to_string());
        assert_eq!(json["opt"], w.opt.unwrap().as_uuid().to_string());
        assert_eq!(
            json["opt_session"],
            w.opt_session.unwrap().as_uuid().to_string()
        );
    }

    #[test]
    fn round_trip_preserves_values() {
        let w = sample();
        let back: Wire = serde_json::from_str(&serde_json::to_string(&w).unwrap()).unwrap();
        assert_eq!(back, w);
    }

    #[test]
    fn none_stream_id_round_trips_as_null() {
        let mut w = sample();
        w.opt = None;
        let json = serde_json::to_value(&w).unwrap();
        assert!(json["opt"].is_null());
        let back: Wire = serde_json::from_value(json).unwrap();
        assert_eq!(back, w);
    }

    #[test]
    fn option_session_id_round_trips_null_and_tolerates_missing_field() {
        let mut w = sample();
        w.opt_session = None;
        let mut json = serde_json::to_value(&w).unwrap();
        assert!(json["opt_session"].is_null());
        assert_eq!(serde_json::from_value::<Wire>(json.clone()).unwrap(), w);
        json.as_object_mut().unwrap().remove("opt_session");
        assert_eq!(serde_json::from_value::<Wire>(json).unwrap(), w);
    }

    #[test]
    fn invalid_priority_is_rejected() {
        let mut json = serde_json::to_value(sample()).unwrap();
        json["priority"] = 0.into();
        assert!(serde_json::from_value::<Wire>(json.clone()).is_err());
        json["priority"] = 7.into();
        json["map"]["a"] = 0.into();
        assert!(serde_json::from_value::<Wire>(json).is_err());
    }
}
