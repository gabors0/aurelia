//! libmpv events as mpv IPC messages, so the IPC translator serves the
//! embedded backend too (mpv's own IPC builds its messages the same way).

use std::ffi::{CStr, c_char, c_int};

use serde_json::{Map, Value, json};

use super::libmpv::{self as ffi, Lib, Node, RawEvent};

/// The IPC message for `event`, or `None` for events the translator doesn't
/// use (log messages and command replies are handled by the caller).
///
/// # Safety
/// `event` must come from `mpv_wait_event` and not have been replaced by a
/// later wait.
pub(crate) unsafe fn to_message(lib: &Lib, event: &RawEvent) -> Option<Value> {
    let name = match event.event_id {
        ffi::EVENT_PROPERTY_CHANGE => {
            // SAFETY: property-change events carry an mpv_event_property.
            let property = unsafe { &*(event.data as *const ffi::EventProperty) };
            // SAFETY: observed with FORMAT_NODE, so data is an mpv_node (or
            // null with FORMAT_NONE when unavailable).
            let data = if property.format == ffi::FORMAT_NODE && !property.data.is_null() {
                unsafe { node_to_json(&*(property.data as *const Node)) }
            } else {
                Value::Null
            };
            return Some(json!({
                "event": "property-change",
                "id": event.reply_userdata,
                // SAFETY: a valid C string for the event's lifetime.
                "name": unsafe { text(property.name) },
                "data": data,
            }));
        }
        ffi::EVENT_END_FILE => {
            // SAFETY: end-file events carry an mpv_event_end_file.
            let end = unsafe { &*(event.data as *const ffi::EventEndFile) };
            let mut message = json!({ "event": "end-file", "reason": end_reason(end.reason) });
            if end.reason == ffi::END_FILE_ERROR {
                message["file_error"] = json!(lib.error_string(end.error));
            }
            return Some(message);
        }
        ffi::EVENT_START_FILE => "start-file",
        ffi::EVENT_FILE_LOADED => "file-loaded",
        ffi::EVENT_SEEK => "seek",
        ffi::EVENT_PLAYBACK_RESTART => "playback-restart",
        ffi::EVENT_IDLE => "idle",
        ffi::EVENT_SHUTDOWN => "shutdown",
        _ => return None,
    };
    Some(json!({ "event": name }))
}

/// mpv's IPC names for end-file reasons.
pub(crate) fn end_reason(reason: c_int) -> &'static str {
    match reason {
        ffi::END_FILE_EOF => "eof",
        ffi::END_FILE_STOP => "stop",
        ffi::END_FILE_QUIT => "quit",
        ffi::END_FILE_ERROR => "error",
        ffi::END_FILE_REDIRECT => "redirect",
        _ => "unknown",
    }
}

/// # Safety
/// `pointer` is null or a valid C string.
unsafe fn text(pointer: *const c_char) -> String {
    if pointer.is_null() {
        return String::new();
    }
    // SAFETY: the caller's contract.
    unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned()
}

/// # Safety
/// `node` must be a valid mpv_node tree.
pub(crate) unsafe fn node_to_json(node: &Node) -> Value {
    // SAFETY (all arms): the union member read is the one `format` names.
    unsafe {
        match node.format {
            ffi::FORMAT_STRING => Value::String(text(node.u.string)),
            ffi::FORMAT_FLAG => Value::Bool(node.u.flag != 0),
            ffi::FORMAT_INT64 => json!(node.u.int64),
            ffi::FORMAT_DOUBLE => serde_json::Number::from_f64(node.u.double)
                .map(Value::Number)
                .unwrap_or(Value::Null),
            ffi::FORMAT_NODE_ARRAY => {
                let list = node.u.list;
                if list.is_null() {
                    return Value::Array(Vec::new());
                }
                let list = &*list;
                (0..list.num.max(0) as usize)
                    .map(|i| node_to_json(&*list.values.add(i)))
                    .collect()
            }
            ffi::FORMAT_NODE_MAP => {
                let list = node.u.list;
                if list.is_null() {
                    return Value::Object(Map::new());
                }
                let list = &*list;
                (0..list.num.max(0) as usize)
                    .map(|i| (text(*list.keys.add(i)), node_to_json(&*list.values.add(i))))
                    .collect::<Map<_, _>>()
                    .into()
            }
            // Unavailable, byte arrays and anything newer.
            ffi::FORMAT_NONE => Value::Null,
            _ => Value::Null,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::CString;

    use super::*;
    use crate::embedded::libmpv::{NodeList, NodeValue};

    fn int(value: i64) -> Node {
        Node {
            u: NodeValue { int64: value },
            format: ffi::FORMAT_INT64,
        }
    }

    #[test]
    fn scalars() {
        let s = CString::new("sub").unwrap();
        let string = Node {
            u: NodeValue {
                string: s.as_ptr() as *mut c_char,
            },
            format: ffi::FORMAT_STRING,
        };
        let flag = Node {
            u: NodeValue { flag: 1 },
            format: ffi::FORMAT_FLAG,
        };
        let double = Node {
            u: NodeValue { double: 12.5 },
            format: ffi::FORMAT_DOUBLE,
        };
        let nan = Node {
            u: NodeValue { double: f64::NAN },
            format: ffi::FORMAT_DOUBLE,
        };
        let none = Node {
            u: NodeValue { int64: 0 },
            format: ffi::FORMAT_NONE,
        };
        unsafe {
            assert_eq!(node_to_json(&string), json!("sub"));
            assert_eq!(node_to_json(&flag), json!(true));
            assert_eq!(node_to_json(&int(7)), json!(7));
            assert_eq!(node_to_json(&double), json!(12.5));
            assert_eq!(node_to_json(&nan), Value::Null);
            assert_eq!(node_to_json(&none), Value::Null);
        }
    }

    #[test]
    fn a_track_list_like_tree() {
        // [{"id": 2, "ff-index": 3}]
        let keys = [
            CString::new("id").unwrap(),
            CString::new("ff-index").unwrap(),
        ];
        let mut key_ptrs: Vec<*mut c_char> = keys.iter().map(|k| k.as_ptr() as *mut _).collect();
        let mut values = [int(2), int(3)];
        let mut map_list = NodeList {
            num: 2,
            values: values.as_mut_ptr(),
            keys: key_ptrs.as_mut_ptr(),
        };
        let mut entries = [Node {
            u: NodeValue {
                list: &raw mut map_list,
            },
            format: ffi::FORMAT_NODE_MAP,
        }];
        let mut array_list = NodeList {
            num: 1,
            values: entries.as_mut_ptr(),
            keys: std::ptr::null_mut(),
        };
        let array = Node {
            u: NodeValue {
                list: &raw mut array_list,
            },
            format: ffi::FORMAT_NODE_ARRAY,
        };
        let value = unsafe { node_to_json(&array) };
        assert_eq!(value, json!([{"id": 2, "ff-index": 3}]));
        let tracks = crate::mpv::ipc::parse_tracks(&value);
        assert_eq!(tracks[0].id, 2);
        assert_eq!(tracks[0].ff_index, Some(3));
    }

    #[test]
    fn end_reasons_use_ipc_names() {
        assert_eq!(end_reason(ffi::END_FILE_EOF), "eof");
        assert_eq!(end_reason(ffi::END_FILE_STOP), "stop");
        assert_eq!(end_reason(ffi::END_FILE_QUIT), "quit");
        assert_eq!(end_reason(ffi::END_FILE_ERROR), "error");
        assert_eq!(end_reason(ffi::END_FILE_REDIRECT), "redirect");
    }

    #[test]
    fn empty_lists() {
        let array = Node {
            u: NodeValue {
                list: std::ptr::null_mut(),
            },
            format: ffi::FORMAT_NODE_ARRAY,
        };
        assert_eq!(unsafe { node_to_json(&array) }, json!([]));
    }
}
