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

use embassy_time::Duration;

use crate::error::Error;
use crate::tlv::TLVTag;

/// GenericSwitch cluster ID.
pub const CLUSTER_ID: u32 = 0x003B;

/// Default long press threshold in milliseconds.
pub const DEFAULT_LONG_PRESS_THRESHOLD_MS: u64 = 500;

/// Default multi-press window in milliseconds.
/// Time allowed between presses to count as a multi-press sequence.
pub const DEFAULT_MULTI_PRESS_WINDOW_MS: u64 = 300;

/// Default maximum multi-press count.
pub const DEFAULT_MULTI_PRESS_MAX: u8 = 3;

/// Input event from hardware (button press/release).
///
/// This represents the raw input from the switch hardware that
/// the state machine processes to generate Matter events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum InputEvent {
    /// Button was pressed (position typically goes to 1).
    Press(u8),
    /// Button was released (position typically goes to 0).
    Release(u8),
    /// Latching switch changed position (for toggle switches).
    LatchChange(u8),
}

/// Internal switch state for the state machine.
///
/// Tracks whether the switch is idle, pressed, in a long press state, or in a multi-press window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum SwitchState {
    /// Switch is idle (not pressed).
    #[default]
    Idle,
    /// Switch was just pressed, waiting to see if it's a long press.
    /// Contains the pressed position.
    Pressed(u8),
    /// Switch has been held past the long press threshold.
    /// Contains the pressed position.
    LongPressed(u8),
    /// Switch was released, waiting in multi-press window.
    /// Contains (position, current_press_count).
    MultiPressWindow(u8, u8),
    /// Switch is pressed during a multi-press sequence.
    /// Contains (position, current_press_count).
    MultiPressPressed(u8, u8),
}

/// Matter events that should be emitted by the handler.
///
/// These are the Matter events that the state machine determines should be
/// sent to subscribers based on the input events and timing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum MatterSwitchEvent {
    /// InitialPress event - button was pressed down.
    InitialPress(u8),
    /// ShortRelease event - button released before long press threshold.
    ShortRelease(u8),
    /// LongPress event - button held past long press threshold.
    LongPress(u8),
    /// LongRelease event - button released after being in long press state.
    LongRelease(u8),
    /// SwitchLatched event - latching switch changed position.
    SwitchLatched(u8),
    /// MultiPressOngoing event - multi-press sequence in progress.
    /// Contains (position, current_count).
    MultiPressOngoing(u8, u8),
    /// MultiPressComplete event - multi-press sequence completed.
    /// Contains (position, total_count).
    MultiPressComplete(u8, u8),
}

/// Configuration for GenericSwitch timing behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct GenericSwitchConfig {
    /// Duration before a press is considered a "long press".
    /// After this threshold, LongPress event is emitted.
    pub long_press_threshold: Duration,
    /// Duration of the multi-press detection window.
    /// A second press within this window starts a multi-press sequence.
    pub multi_press_window: Duration,
    /// Maximum number of presses to count in a multi-press sequence.
    /// When this count is reached, MultiPressComplete is emitted immediately.
    pub multi_press_max: u8,
    /// Whether multi-press detection is enabled.
    /// When disabled, ShortRelease goes directly to Idle.
    pub multi_press_enabled: bool,
}

impl Default for GenericSwitchConfig {
    fn default() -> Self {
        Self {
            long_press_threshold: Duration::from_millis(DEFAULT_LONG_PRESS_THRESHOLD_MS),
            multi_press_window: Duration::from_millis(DEFAULT_MULTI_PRESS_WINDOW_MS),
            multi_press_max: DEFAULT_MULTI_PRESS_MAX,
            multi_press_enabled: true,
        }
    }
}

impl GenericSwitchConfig {
    /// Create a new configuration with custom long press threshold (multi-press enabled).
    pub const fn with_long_press_threshold(threshold: Duration) -> Self {
        Self {
            long_press_threshold: threshold,
            multi_press_window: Duration::from_millis(DEFAULT_MULTI_PRESS_WINDOW_MS),
            multi_press_max: DEFAULT_MULTI_PRESS_MAX,
            multi_press_enabled: true,
        }
    }

    /// Create a configuration with multi-press disabled (simple short/long press only).
    pub const fn without_multi_press() -> Self {
        Self {
            long_press_threshold: Duration::from_millis(DEFAULT_LONG_PRESS_THRESHOLD_MS),
            multi_press_window: Duration::from_millis(DEFAULT_MULTI_PRESS_WINDOW_MS),
            multi_press_max: DEFAULT_MULTI_PRESS_MAX,
            multi_press_enabled: false,
        }
    }

    /// Create a full custom configuration.
    pub const fn new(
        long_press_threshold: Duration,
        multi_press_window: Duration,
        multi_press_max: u8,
        multi_press_enabled: bool,
    ) -> Self {
        Self {
            long_press_threshold,
            multi_press_window,
            multi_press_max,
            multi_press_enabled,
        }
    }
}

/// Process an input event and return the Matter event(s) to emit.
///
/// This is a pure function that handles the state machine logic for
/// long press and multi-press detection. It takes the current state, config, and input event,
/// returning the new state and any Matter events to emit.
///
/// # Arguments
/// - `state`: Current switch state
/// - `config`: Switch configuration (for multi-press settings)
/// - `event`: Input event from hardware
/// - `long_press_elapsed`: Whether the long press timer has elapsed
///
/// # Returns
/// Tuple of (new_state, events_to_emit)
///
/// # State Machine
///
/// ```text
/// Without multi-press:
///   IDLE → PRESSED (emit InitialPress)
///   PRESSED + release → IDLE (emit ShortRelease)
///   PRESSED + timer → LONG_PRESSED (emit LongPress)
///   LONG_PRESSED + release → IDLE (emit LongRelease)
///
/// With multi-press:
///   IDLE → PRESSED (emit InitialPress)
///   PRESSED + release → MULTI_PRESS_WINDOW (emit ShortRelease, start window timer)
///   MULTI_PRESS_WINDOW + press → MULTI_PRESS_PRESSED (emit MultiPressOngoing if count > 1)
///   MULTI_PRESS_PRESSED + release → MULTI_PRESS_WINDOW or IDLE if max reached
///   MULTI_PRESS_WINDOW + timeout → IDLE (emit MultiPressComplete)
/// ```
pub fn process_switch_event(
    state: SwitchState,
    config: &GenericSwitchConfig,
    event: InputEvent,
    long_press_elapsed: bool,
) -> (SwitchState, heapless::Vec<MatterSwitchEvent, 2>) {
    let mut events = heapless::Vec::new();

    let new_state = match (state, event) {
        // Idle + Press -> Pressed, emit InitialPress
        (SwitchState::Idle, InputEvent::Press(pos)) => {
            events.push(MatterSwitchEvent::InitialPress(pos)).ok();
            SwitchState::Pressed(pos)
        }

        // Idle + LatchChange -> Idle, emit SwitchLatched
        (SwitchState::Idle, InputEvent::LatchChange(pos)) => {
            events.push(MatterSwitchEvent::SwitchLatched(pos)).ok();
            SwitchState::Idle
        }

        // Pressed + Release (before long press timer)
        (SwitchState::Pressed(pos), InputEvent::Release(_)) if !long_press_elapsed => {
            events.push(MatterSwitchEvent::ShortRelease(pos)).ok();
            if config.multi_press_enabled {
                // Start multi-press window with count=1
                SwitchState::MultiPressWindow(pos, 1)
            } else {
                SwitchState::Idle
            }
        }

        // Pressed + Release (long press timer already elapsed - this shouldn't happen normally)
        (SwitchState::Pressed(pos), InputEvent::Release(_)) if long_press_elapsed => {
            // Timer already elapsed, so this is a long release
            events.push(MatterSwitchEvent::LongRelease(pos)).ok();
            SwitchState::Idle
        }

        // LongPressed + Release -> Idle, emit LongRelease
        (SwitchState::LongPressed(pos), InputEvent::Release(_)) => {
            events.push(MatterSwitchEvent::LongRelease(pos)).ok();
            SwitchState::Idle
        }

        // MultiPressWindow + Press -> MultiPressPressed, increment count
        (SwitchState::MultiPressWindow(pos, count), InputEvent::Press(new_pos))
            if new_pos == pos =>
        {
            let new_count = count + 1;
            // Emit MultiPressOngoing for counts > 1
            if new_count > 1 {
                events
                    .push(MatterSwitchEvent::MultiPressOngoing(pos, new_count))
                    .ok();
            }
            SwitchState::MultiPressPressed(pos, new_count)
        }

        // MultiPressPressed + Release
        (SwitchState::MultiPressPressed(pos, count), InputEvent::Release(_)) => {
            if count >= config.multi_press_max {
                // Max count reached, emit complete
                events
                    .push(MatterSwitchEvent::MultiPressComplete(pos, count))
                    .ok();
                SwitchState::Idle
            } else {
                // Wait for more presses
                SwitchState::MultiPressWindow(pos, count)
            }
        }

        // Stay in current state for other combinations
        (state, _) => state,
    };

    (new_state, events)
}

/// Process a multi-press window timeout.
///
/// Call this when the multi-press window timer fires. If in MultiPressWindow state,
/// this emits MultiPressComplete and returns to Idle.
///
/// # Returns
/// Tuple of (new_state, optional_event)
pub fn process_multi_press_timeout(state: SwitchState) -> (SwitchState, Option<MatterSwitchEvent>) {
    match state {
        SwitchState::MultiPressWindow(pos, count) => (
            SwitchState::Idle,
            Some(MatterSwitchEvent::MultiPressComplete(pos, count)),
        ),
        // Timer fired but we're not in multi-press window
        _ => (state, None),
    }
}

/// Process a long press timer expiration.
///
/// Call this when the long press timer fires while in the Pressed state.
/// Returns the new state and the LongPress event to emit.
pub fn process_long_press_timer(state: SwitchState) -> (SwitchState, Option<MatterSwitchEvent>) {
    match state {
        SwitchState::Pressed(pos) => (
            SwitchState::LongPressed(pos),
            Some(MatterSwitchEvent::LongPress(pos)),
        ),
        // Timer fired but we're not in Pressed state (already released)
        _ => (state, None),
    }
}

/// GenericSwitch cluster event IDs per Matter Application Cluster Specification.
pub mod events {
    /// SwitchLatched event (0x00)
    ///
    /// Generated when a latching switch changes position.
    /// Payload: `{ NewPosition [0]: u8 }`
    ///
    /// Use [`encode_switch_latched`] to encode the payload.
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
    /// Use `process_long_press_timer()` to detect when to emit this event.
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
    /// Use `process_switch_event()` with `SwitchState::LongPressed` to detect.
    pub const LONG_RELEASE: u32 = 0x04;

    /// MultiPressOngoing event (0x05)
    ///
    /// Generated during a multi-press sequence (double-click, triple-click, etc.)
    /// to indicate the current count.
    /// Payload: `{ NewPosition [0]: u8, CurrentCount [1]: u8 }`
    ///
    /// Use `process_switch_event()` with multi-press enabled config to detect.
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

/// Encode a SwitchLatched event payload.
///
/// Used for latching switches (toggles) when the position changes.
///
/// # Arguments
/// - `new_position`: The new switch position after the latch.
///
/// # Returns
/// Pre-encoded TLV payload for the event.
///
/// # Example
///
/// ```ignore
/// use rs_matter::dm::clusters::generic_switch::{events, encode_switch_latched, CLUSTER_ID};
/// use rs_matter::dm::types::{PendingEvent, EventNumberGenerator};
/// use rs_matter::im::EventPriority;
///
/// let event_gen = EventNumberGenerator::new();
///
/// // When toggle switch changes position
/// fn on_toggle_changed(endpoint_id: u16, new_position: u8, event_gen: &EventNumberGenerator) -> PendingEvent {
///     let payload: heapless::Vec<u8, 16> = encode_switch_latched(new_position);
///     PendingEvent::with_payload(
///         endpoint_id,
///         CLUSTER_ID,
///         events::SWITCH_LATCHED,
///         event_gen.next(),
///         EventPriority::Info,
///         get_system_time_ms(),
///         &payload,
///     )
/// }
/// ```
pub fn encode_switch_latched<const N: usize>(new_position: u8) -> heapless::Vec<u8, N> {
    encode_initial_press(new_position)
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
/// Use `process_long_press_timer()` to detect when to emit this event.
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
/// Use `process_switch_event()` with `SwitchState::LongPressed` to detect.
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
/// Use `process_switch_event()` with multi-press enabled config to detect when to emit.
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

    #[test]
    fn test_encode_switch_latched() {
        let payload: heapless::Vec<u8, 16> = encode_switch_latched(1);
        // Should contain TLV: struct { u8(tag=0, value=1) }
        assert!(!payload.is_empty());
        // Verify it's the same as encode_initial_press since they have same format
        let initial_press: heapless::Vec<u8, 16> = encode_initial_press(1);
        assert_eq!(payload.as_slice(), initial_press.as_slice());
    }

    // State machine tests

    #[test]
    fn test_state_machine_short_press_no_multi() {
        let config = GenericSwitchConfig::without_multi_press();

        // IDLE -> Press -> PRESSED (emit InitialPress)
        let (state, events) =
            process_switch_event(SwitchState::Idle, &config, InputEvent::Press(1), false);
        assert_eq!(state, SwitchState::Pressed(1));
        assert_eq!(events.len(), 1);
        assert_eq!(events[0], MatterSwitchEvent::InitialPress(1));

        // PRESSED -> Release (before timer) -> IDLE (emit ShortRelease)
        let (state, events) = process_switch_event(state, &config, InputEvent::Release(0), false);
        assert_eq!(state, SwitchState::Idle);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0], MatterSwitchEvent::ShortRelease(1));
    }

    #[test]
    fn test_state_machine_long_press() {
        let config = GenericSwitchConfig::without_multi_press();

        // IDLE -> Press -> PRESSED
        let (state, events) =
            process_switch_event(SwitchState::Idle, &config, InputEvent::Press(1), false);
        assert_eq!(state, SwitchState::Pressed(1));
        assert_eq!(events[0], MatterSwitchEvent::InitialPress(1));

        // Timer fires -> LONG_PRESSED (emit LongPress)
        let (state, event) = process_long_press_timer(state);
        assert_eq!(state, SwitchState::LongPressed(1));
        assert_eq!(event, Some(MatterSwitchEvent::LongPress(1)));

        // LONG_PRESSED -> Release -> IDLE (emit LongRelease)
        let (state, events) = process_switch_event(state, &config, InputEvent::Release(0), false);
        assert_eq!(state, SwitchState::Idle);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0], MatterSwitchEvent::LongRelease(1));
    }

    #[test]
    fn test_state_machine_latch_change() {
        let config = GenericSwitchConfig::default();

        // IDLE -> LatchChange -> IDLE (emit SwitchLatched)
        let (state, events) =
            process_switch_event(SwitchState::Idle, &config, InputEvent::LatchChange(1), false);
        assert_eq!(state, SwitchState::Idle);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0], MatterSwitchEvent::SwitchLatched(1));
    }

    #[test]
    fn test_state_machine_timer_in_wrong_state() {
        // Timer fires when IDLE - should do nothing
        let (state, event) = process_long_press_timer(SwitchState::Idle);
        assert_eq!(state, SwitchState::Idle);
        assert_eq!(event, None);

        // Timer fires when already LONG_PRESSED - should do nothing
        let (state, event) = process_long_press_timer(SwitchState::LongPressed(1));
        assert_eq!(state, SwitchState::LongPressed(1));
        assert_eq!(event, None);
    }

    #[test]
    fn test_state_machine_double_press() {
        let config = GenericSwitchConfig::default(); // multi-press enabled

        // First press
        let (state, events) =
            process_switch_event(SwitchState::Idle, &config, InputEvent::Press(1), false);
        assert_eq!(state, SwitchState::Pressed(1));
        assert_eq!(events[0], MatterSwitchEvent::InitialPress(1));

        // First release -> MultiPressWindow
        let (state, events) = process_switch_event(state, &config, InputEvent::Release(0), false);
        assert_eq!(state, SwitchState::MultiPressWindow(1, 1));
        assert_eq!(events[0], MatterSwitchEvent::ShortRelease(1));

        // Second press -> MultiPressPressed, emit MultiPressOngoing
        let (state, events) = process_switch_event(state, &config, InputEvent::Press(1), false);
        assert_eq!(state, SwitchState::MultiPressPressed(1, 2));
        assert_eq!(events.len(), 1);
        assert_eq!(events[0], MatterSwitchEvent::MultiPressOngoing(1, 2));

        // Second release -> back to MultiPressWindow (count < max)
        let (state, events) = process_switch_event(state, &config, InputEvent::Release(0), false);
        assert_eq!(state, SwitchState::MultiPressWindow(1, 2));
        assert!(events.is_empty()); // No events until timeout or max reached

        // Timeout -> emit MultiPressComplete
        let (state, event) = process_multi_press_timeout(state);
        assert_eq!(state, SwitchState::Idle);
        assert_eq!(event, Some(MatterSwitchEvent::MultiPressComplete(1, 2)));
    }

    #[test]
    fn test_state_machine_triple_press_max_reached() {
        let config = GenericSwitchConfig::new(
            Duration::from_millis(500),
            Duration::from_millis(300),
            3, // max 3 presses
            true,
        );

        // First press-release
        let (state, _) =
            process_switch_event(SwitchState::Idle, &config, InputEvent::Press(1), false);
        let (state, _) = process_switch_event(state, &config, InputEvent::Release(0), false);
        assert_eq!(state, SwitchState::MultiPressWindow(1, 1));

        // Second press-release
        let (state, _) = process_switch_event(state, &config, InputEvent::Press(1), false);
        let (state, _) = process_switch_event(state, &config, InputEvent::Release(0), false);
        assert_eq!(state, SwitchState::MultiPressWindow(1, 2));

        // Third press
        let (state, events) = process_switch_event(state, &config, InputEvent::Press(1), false);
        assert_eq!(state, SwitchState::MultiPressPressed(1, 3));
        assert_eq!(events[0], MatterSwitchEvent::MultiPressOngoing(1, 3));

        // Third release -> max reached, emit complete immediately
        let (state, events) = process_switch_event(state, &config, InputEvent::Release(0), false);
        assert_eq!(state, SwitchState::Idle);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0], MatterSwitchEvent::MultiPressComplete(1, 3));
    }

    #[test]
    fn test_multi_press_timeout_wrong_state() {
        // Timeout when IDLE - should do nothing
        let (state, event) = process_multi_press_timeout(SwitchState::Idle);
        assert_eq!(state, SwitchState::Idle);
        assert_eq!(event, None);

        // Timeout when pressed - should do nothing
        let (state, event) = process_multi_press_timeout(SwitchState::Pressed(1));
        assert_eq!(state, SwitchState::Pressed(1));
        assert_eq!(event, None);
    }

    #[test]
    fn test_config_default() {
        let config = GenericSwitchConfig::default();
        assert_eq!(
            config.long_press_threshold,
            Duration::from_millis(DEFAULT_LONG_PRESS_THRESHOLD_MS)
        );
        assert_eq!(
            config.multi_press_window,
            Duration::from_millis(DEFAULT_MULTI_PRESS_WINDOW_MS)
        );
        assert_eq!(config.multi_press_max, DEFAULT_MULTI_PRESS_MAX);
        assert!(config.multi_press_enabled);
    }

    #[test]
    fn test_config_without_multi_press() {
        let config = GenericSwitchConfig::without_multi_press();
        assert!(!config.multi_press_enabled);
    }

    #[test]
    fn test_config_custom() {
        let config = GenericSwitchConfig::with_long_press_threshold(Duration::from_millis(1000));
        assert_eq!(config.long_press_threshold, Duration::from_millis(1000));
        assert!(config.multi_press_enabled);
    }
}
