//! The value form of `TXXX:Podcast Value Routes` from the routes of the
//! parser (musicindex ADR 0101 R-TAG-4 item 1 and R-TAG-6).

use serde_json::{Map, Value, json};
use stophammer_parser::{IngestPaymentRoute, IngestValueBlock};

/// Gives the first Lightning `podcast:value` block of one parent (an item or
/// a channel) in the value form of `TXXX:Podcast Value Routes`: a JSON array
/// with one object for each recipient. `None` when the parent has no
/// Lightning block.
///
/// `blocks` are the `podcast:value` blocks of the parent, and `routes` are
/// their recipients. A Lightning block with no recipient gives an empty
/// array, because the parent has a block (musicindex ADR 0101 R-TAG-6
/// item 1). When `blocks` is empty, the function finds the blocks from
/// `routes`, for values that a parser before `value_blocks` made.
///
/// A block with no type or with the type `lightning` is a Lightning block.
/// The function copies the recipients of that block in feed order. It does
/// not make a route and does not remove a recipient (R-TAG-6 item 3).
///
/// Each object has the fields `recipient_name`, `route_type`, `split`, `fee`,
/// `address`, `custom_key` and `custom_value`. `route_type` is the `type`
/// text in lower case. `split` is the `split` text as a number, or `null`
/// when the text is not a number.
#[must_use]
pub fn value_routes(blocks: &[IngestValueBlock], routes: &[IngestPaymentRoute]) -> Option<Value> {
    let block = if blocks.is_empty() {
        routes
            .iter()
            .filter(|route| is_lightning_block(&route.value_type))
            .map(|route| route.block_index)
            .min()?
    } else {
        blocks
            .iter()
            .filter(|block| is_lightning_block(&block.value_type))
            .map(|block| block.block_index)
            .min()?
    };
    Some(recipients(routes.iter().filter(|route| {
        route.block_index == block && is_lightning_block(&route.value_type)
    })))
}

/// Gives each route of `routes` in the value form, in the given order.
pub(crate) fn recipients<'a>(routes: impl Iterator<Item = &'a IngestPaymentRoute>) -> Value {
    Value::Array(routes.map(recipient).collect())
}

fn is_lightning_block(value_type: &str) -> bool {
    let value_type = value_type.trim();
    value_type.is_empty() || value_type.eq_ignore_ascii_case("lightning")
}

fn recipient(route: &IngestPaymentRoute) -> Value {
    let text = |value: Option<&str>| {
        value
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map_or(Value::Null, |value| json!(value))
    };
    let split = route
        .split_text
        .as_deref()
        .and_then(musicindex_value::parse_share)
        .filter(|split| split.is_finite())
        .map_or(Value::Null, |split| json!(split));
    let mut object = Map::new();
    object.insert(
        "recipient_name".into(),
        text(route.recipient_name.as_deref()),
    );
    object.insert(
        "route_type".into(),
        text(
            route
                .type_text
                .as_deref()
                .map(str::to_ascii_lowercase)
                .as_deref(),
        ),
    );
    object.insert("split".into(), split);
    object.insert("fee".into(), json!(route.fee));
    object.insert("address".into(), text(Some(&route.address)));
    object.insert("custom_key".into(), text(route.custom_key.as_deref()));
    object.insert("custom_value".into(), text(route.custom_value.as_deref()));
    Value::Object(object)
}
