/*
 *
 *    Copyright (c) 2020-2022 Project CHIP Authors
 *
 *    Licensed under the Apache License, Version 2.0 (the "License");
 *    you may not use this file except in compliance with the License.
 *    You may obtain a copy of the License at
 *
 *        http://www.apache.org/licenses/LICENSE-2.0
 *
 *    Unless required by applicable law or agreed to in writing, software
 *    distributed under the License is distributed on an "AS IS" BASIS,
 *    WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 *    See the License for the specific language governing permissions and
 *    limitations under the License.
 */

//! GenericSwitch cluster (0x003B) definitions.
//!
//! This cluster provides an interface for observing switch inputs.
//! It is primarily used for momentary switches (buttons) and latching switches (toggles).
//!
//! # Events
//!
//! GenericSwitch is an event-centric cluster. The primary functionality is delivered
//! through events like `InitialPress`, `ShortRelease`, `LongPress`, etc.
//!
//! # Usage
//!
//! To implement a GenericSwitch handler that emits events:
//!
//! ```ignore
//! use rs_matter::dm::clusters::generic_switch::events;
//! use rs_matter::dm::types::{EventSource, PendingEvent, EventNumberGenerator};
//!
//! struct MySwitchHandler {
//!     event_queue: Mutex<heapless::Vec<PendingEvent, 16>>,
//!     event_gen: EventNumberGenerator,
//! }
//!
//! impl MySwitchHandler {
//!     fn on_button_press(&self, position: u8) {
//!         let event = PendingEvent::with_payload(
//!             self.endpoint_id,
//!             CLUSTER_ID,
//!             events::INITIAL_PRESS,
//!             self.event_gen.next(),
//!             EventPriority::Info,
//!             get_system_time_ms(),
//!             &encode_initial_press(position),
//!         );
//!         self.event_queue.lock().push(event).ok();
//!     }
//! }
//!
//! impl EventSource for MySwitchHandler {
//!     fn take_pending_events(&self) -> heapless::Vec<PendingEvent, 16> {
//!         core::mem::take(&mut *self.event_queue.lock())
//!     }
//!
//!     fn has_pending_events(&self) -> bool {
//!         !self.event_queue.lock().is_empty()
//!     }
//! }
//! ```

use crate::error::Error;
use crate::tlv::TLVTag;

/// GenericSwitch cluster ID.
pub const CLUSTER_ID: u32 = 0x003B;

/// GenericSwitch cluster event IDs per Matter Application Cluster Specification.
pub mod events {
    /// SwitchLatched event (0x00)
    ///
    /// Generated when a latching switch changes position.
    /// Payload: `{ NewPosition [0]: u8 }`
    ///
    /// TODO: Implement for latching switch support.
    pub const SWITCH_LATCHED: u32 = 0x00;

    /// InitialPress event (0x01)
    ///
    /// Generated when a momentary switch is pressed down.
    /// This is the first event in any button press sequence.
    /// Payload: `{ NewPosition [0]: u8 }`
    pub const INITIAL_PRESS: u32 = 0x01;

    /// LongPress event (0x02)
    ///
    /// Generated when a momentary switch is held past the long press threshold.
    /// Only generated if the switch supports long press detection.
    /// Payload: `{ NewPosition [0]: u8 }`
    ///
    /// TODO: Implement long press timing detection.
    pub const LONG_PRESS: u32 = 0x02;

    /// ShortRelease event (0x03)
    ///
    /// Generated when a momentary switch is released after a short press
    /// (before the long press threshold).
    /// Payload: `{ PreviousPosition [0]: u8 }`
    pub const SHORT_RELEASE: u32 = 0x03;

    /// LongRelease event (0x04)
    ///
    /// Generated when a momentary switch is released after a long press.
    /// Payload: `{ PreviousPosition [0]: u8 }`
    ///
    /// TODO: Implement long press timing detection.
    pub const LONG_RELEASE: u32 = 0x04;

    /// MultiPressOngoing event (0x05)
    ///
    /// Generated during a multi-press sequence (double-click, triple-click, etc.)
    /// to indicate the current count.
    /// Payload: `{ NewPosition [0]: u8, CurrentCount [1]: u8 }`
    ///
    /// TODO: Implement multi-press timing detection.
    pub const MULTI_PRESS_ONGOING: u32 = 0x05;

    /// MultiPressComplete event (0x06)
    ///
    /// Generated when a multi-press sequence completes.
    /// Payload: `{ PreviousPosition [0]: u8, TotalCount [1]: u8 }`
    pub const MULTI_PRESS_COMPLETE: u32 = 0x06;
}

/// GenericSwitch attribute IDs.
pub mod attributes {
    /// Number of switch positions (required).
    /// Type: u8, Range: 2-255
    pub const NUMBER_OF_POSITIONS: u32 = 0x0000;

    /// Current switch position (required).
    /// Type: u8, Range: 0 to (NumberOfPositions - 1)
    pub const CURRENT_POSITION: u32 = 0x0001;

    /// Multi-press maximum count (optional).
    /// Type: u8, Range: 2-255
    /// Only present if MultiPress feature is supported.
    pub const MULTI_PRESS_MAX: u32 = 0x0002;
}

/// GenericSwitch feature flags.
pub mod features {
    /// Latching Switch (LS) - Switch maintains position mechanically.
    pub const LATCHING_SWITCH: u32 = 0x0001;

    /// Momentary Switch (MS) - Switch returns to default position when released.
    pub const MOMENTARY_SWITCH: u32 = 0x0002;

    /// Momentary Switch Release (MSR) - Supports ShortRelease/LongRelease events.
    pub const MOMENTARY_SWITCH_RELEASE: u32 = 0x0004;

    /// Momentary Switch Long Press (MSL) - Supports LongPress event.
    pub const MOMENTARY_SWITCH_LONG_PRESS: u32 = 0x0008;

    /// Momentary Switch Multi Press (MSM) - Supports MultiPress events.
    pub const MOMENTARY_SWITCH_MULTI_PRESS: u32 = 0x0010;
}

/// Encode an InitialPress event payload.
///
/// # Arguments
/// - `new_position`: The new switch position (typically 1 for pressed).
///
/// # Returns
/// Pre-encoded TLV payload for the event.
pub fn encode_initial_press<const N: usize>(new_position: u8) -> heapless::Vec<u8, N> {
    let mut buf = heapless::Vec::new();
    // Ensure we have enough space
    if buf.resize_default(N).is_err() {
        return heapless::Vec::new();
    }
    let mut wb = crate::utils::storage::WriteBuf::new(&mut buf);
    // TLV structure: { NewPosition [0]: u8 }
    if encode_position_payload(&mut wb, new_position).is_err() {
        return heapless::Vec::new();
    }
    let len = wb.get_tail();
    drop(wb);
    buf.truncate(len);
    buf
}

/// Encode a ShortRelease event payload.
///
/// # Arguments
/// - `previous_position`: The previous switch position before release.
///
/// # Returns
/// Pre-encoded TLV payload for the event.
pub fn encode_short_release<const N: usize>(previous_position: u8) -> heapless::Vec<u8, N> {
    let mut buf = heapless::Vec::new();
    if buf.resize_default(N).is_err() {
        return heapless::Vec::new();
    }
    let mut wb = crate::utils::storage::WriteBuf::new(&mut buf);
    // TLV structure: { PreviousPosition [0]: u8 }
    if encode_position_payload(&mut wb, previous_position).is_err() {
        return heapless::Vec::new();
    }
    let len = wb.get_tail();
    drop(wb);
    buf.truncate(len);
    buf
}

/// Encode a LongPress event payload.
///
/// # Arguments
/// - `new_position`: The switch position during long press.
///
/// # Returns
/// Pre-encoded TLV payload for the event.
///
/// TODO: Implement long press support.
pub fn encode_long_press<const N: usize>(new_position: u8) -> heapless::Vec<u8, N> {
    encode_initial_press(new_position)
}

/// Encode a LongRelease event payload.
///
/// # Arguments
/// - `previous_position`: The previous switch position before release.
///
/// # Returns
/// Pre-encoded TLV payload for the event.
///
/// TODO: Implement long press support.
pub fn encode_long_release<const N: usize>(previous_position: u8) -> heapless::Vec<u8, N> {
    encode_short_release(previous_position)
}

/// Encode a MultiPressOngoing event payload.
///
/// # Arguments
/// - `new_position`: The current switch position.
/// - `current_count`: The current press count in the sequence.
///
/// # Returns
/// Pre-encoded TLV payload for the event.
///
/// TODO: Implement multi-press support.
pub fn encode_multi_press_ongoing<const N: usize>(
    new_position: u8,
    current_count: u8,
) -> heapless::Vec<u8, N> {
    let mut buf = heapless::Vec::new();
    if buf.resize_default(N).is_err() {
        return heapless::Vec::new();
    }
    let mut wb = crate::utils::storage::WriteBuf::new(&mut buf);
    // TLV structure: { NewPosition [0]: u8, CurrentCount [1]: u8 }
    if encode_multi_press_payload(&mut wb, new_position, current_count).is_err() {
        return heapless::Vec::new();
    }
    let len = wb.get_tail();
    drop(wb);
    buf.truncate(len);
    buf
}

/// Encode a MultiPressComplete event payload.
///
/// # Arguments
/// - `previous_position`: The previous switch position.
/// - `total_count`: The total number of presses in the completed sequence.
///
/// # Returns
/// Pre-encoded TLV payload for the event.
pub fn encode_multi_press_complete<const N: usize>(
    previous_position: u8,
    total_count: u8,
) -> heapless::Vec<u8, N> {
    let mut buf = heapless::Vec::new();
    if buf.resize_default(N).is_err() {
        return heapless::Vec::new();
    }
    let mut wb = crate::utils::storage::WriteBuf::new(&mut buf);
    // TLV structure: { PreviousPosition [0]: u8, TotalCount [1]: u8 }
    if encode_multi_press_payload(&mut wb, previous_position, total_count).is_err() {
        return heapless::Vec::new();
    }
    let len = wb.get_tail();
    drop(wb);
    buf.truncate(len);
    buf
}

/// Internal helper to encode a single-position payload.
fn encode_position_payload(
    wb: &mut crate::utils::storage::WriteBuf<'_>,
    position: u8,
) -> Result<(), Error> {
    use crate::tlv::TLVWrite;
    wb.start_struct(&TLVTag::Anonymous)?;
    wb.u8(&TLVTag::Context(0), position)?;
    wb.end_container()
}

/// Internal helper to encode a multi-press payload.
fn encode_multi_press_payload(
    wb: &mut crate::utils::storage::WriteBuf<'_>,
    position: u8,
    count: u8,
) -> Result<(), Error> {
    use crate::tlv::TLVWrite;
    wb.start_struct(&TLVTag::Anonymous)?;
    wb.u8(&TLVTag::Context(0), position)?;
    wb.u8(&TLVTag::Context(1), count)?;
    wb.end_container()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_ids() {
        assert_eq!(events::SWITCH_LATCHED, 0x00);
        assert_eq!(events::INITIAL_PRESS, 0x01);
        assert_eq!(events::LONG_PRESS, 0x02);
        assert_eq!(events::SHORT_RELEASE, 0x03);
        assert_eq!(events::LONG_RELEASE, 0x04);
        assert_eq!(events::MULTI_PRESS_ONGOING, 0x05);
        assert_eq!(events::MULTI_PRESS_COMPLETE, 0x06);
    }

    #[test]
    fn test_encode_initial_press() {
        let payload: heapless::Vec<u8, 16> = encode_initial_press(1);
        // Should contain TLV: struct { u8(tag=0, value=1) }
        assert!(!payload.is_empty());
    }

    #[test]
    fn test_encode_short_release() {
        let payload: heapless::Vec<u8, 16> = encode_short_release(1);
        assert!(!payload.is_empty());
    }

    #[test]
    fn test_encode_multi_press_complete() {
        let payload: heapless::Vec<u8, 16> = encode_multi_press_complete(1, 2);
        assert!(!payload.is_empty());
    }
}
