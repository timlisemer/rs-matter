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

//! Event source trait and related types for Matter event reporting.
//!
//! This module provides the infrastructure for cluster handlers to emit
//! Matter events that are reported to subscribers.
//!
//! # Overview
//!
//! Cluster handlers that need to emit events implement the [`EventSource`] trait.
//! When events occur (e.g., button press), handlers queue them internally.
//! During subscription reports, the data model calls [`EventSource::take_pending_events`]
//! to collect and report queued events.
//!
//! # Example
//!
//! ```ignore
//! use rs_matter::dm::types::event::{EventSource, PendingEvent, EventPriority};
//!
//! struct MyButtonHandler {
//!     events: Mutex<heapless::Vec<PendingEvent, 16>>,
//!     event_number: AtomicU64,
//! }
//!
//! impl EventSource for MyButtonHandler {
//!     fn take_pending_events(&self) -> heapless::Vec<PendingEvent, 16> {
//!         core::mem::take(&mut *self.events.lock())
//!     }
//!
//!     fn has_pending_events(&self) -> bool {
//!         !self.events.lock().is_empty()
//!     }
//! }
//! ```

use core::sync::atomic::{AtomicU64, Ordering};

use crate::im::{EventPriority, ClusterId, EndptId};

/// Maximum number of pending events per handler.
///
/// This limit prevents unbounded memory growth if events aren't consumed.
/// TODO: Make this configurable or use a different storage strategy.
pub const MAX_PENDING_EVENTS: usize = 16;

/// Maximum size of pre-encoded TLV event payload.
///
/// Most event payloads are small (e.g., GenericSwitch events are ~4 bytes).
/// TODO: Consider dynamic allocation for larger payloads.
pub const MAX_EVENT_PAYLOAD_SIZE: usize = 64;

/// A pending event ready to be reported to subscribers.
///
/// Events are queued by cluster handlers and collected during subscription
/// report generation.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct PendingEvent {
    /// The endpoint ID where the event occurred.
    pub endpoint_id: EndptId,
    /// The cluster ID that generated the event.
    pub cluster_id: ClusterId,
    /// The event ID within the cluster (e.g., 0x01 for GenericSwitch InitialPress).
    pub event_id: u32,
    /// Monotonically increasing event number.
    ///
    /// This number MUST never decrease, even across device restarts.
    /// TODO: Implement persistence for event numbers.
    pub event_number: u64,
    /// Event priority level (Debug, Info, Critical).
    pub priority: EventPriority,
    /// System timestamp in milliseconds since device boot.
    ///
    /// TODO: Support epoch timestamps when device has reliable time.
    pub system_timestamp_ms: u64,
    /// Pre-encoded TLV payload with cluster-specific event data.
    ///
    /// The payload format is defined by the cluster specification.
    /// For example, GenericSwitch InitialPress: `{ NewPosition [0]: u8 }`.
    #[cfg_attr(feature = "defmt", defmt(Debug2Format))]
    pub payload: heapless::Vec<u8, MAX_EVENT_PAYLOAD_SIZE>,
}

impl PendingEvent {
    /// Create a new pending event with the given parameters.
    pub fn new(
        endpoint_id: EndptId,
        cluster_id: ClusterId,
        event_id: u32,
        event_number: u64,
        priority: EventPriority,
        system_timestamp_ms: u64,
    ) -> Self {
        Self {
            endpoint_id,
            cluster_id,
            event_id,
            event_number,
            priority,
            system_timestamp_ms,
            payload: heapless::Vec::new(),
        }
    }

    /// Create a new pending event with payload.
    pub fn with_payload(
        endpoint_id: EndptId,
        cluster_id: ClusterId,
        event_id: u32,
        event_number: u64,
        priority: EventPriority,
        system_timestamp_ms: u64,
        payload: &[u8],
    ) -> Self {
        let mut event = Self::new(
            endpoint_id,
            cluster_id,
            event_id,
            event_number,
            priority,
            system_timestamp_ms,
        );
        // Truncate if payload is too large
        let len = payload.len().min(MAX_EVENT_PAYLOAD_SIZE);
        event.payload.extend_from_slice(&payload[..len]).ok();
        event
    }
}

/// Trait for cluster handlers that can produce Matter events.
///
/// Implement this trait on cluster handlers that need to emit events.
/// The data model will call [`take_pending_events`] during subscription
/// report generation to collect queued events.
///
/// # Thread Safety
///
/// Implementations must be `Send + Sync` as events may be queued from
/// different contexts (e.g., interrupt handlers, async tasks) and consumed
/// from the Matter task.
///
/// # Example
///
/// See module-level documentation for a complete example.
pub trait EventSource: Send + Sync {
    /// Returns pending events and clears the internal queue.
    ///
    /// This method is called during subscription report generation.
    /// Events returned here will be encoded as `EventReportIB` structures
    /// in the `ReportData` response.
    ///
    /// Implementations should atomically drain their event queue and
    /// return all pending events. If no events are pending, return
    /// an empty vector.
    fn take_pending_events(&self) -> heapless::Vec<PendingEvent, MAX_PENDING_EVENTS>;

    /// Check if there are pending events without draining.
    ///
    /// This is used to quickly check if event processing is needed
    /// before calling the more expensive [`take_pending_events`].
    fn has_pending_events(&self) -> bool;

    /// Filter events by minimum event number.
    ///
    /// Returns events with `event_number >= min_event_number`.
    /// This is used to implement `EventFilter` from subscription requests.
    ///
    /// TODO: Implement proper event filtering for EventFilter support.
    /// Currently falls back to returning all pending events.
    fn take_events_since(
        &self,
        _min_event_number: u64,
    ) -> heapless::Vec<PendingEvent, MAX_PENDING_EVENTS> {
        // TODO: Implement event number filtering
        // For now, return all pending events
        self.take_pending_events()
    }
}

/// Global event number generator.
///
/// Provides monotonically increasing event numbers across all clusters.
/// Per Matter spec, event numbers must never decrease, even across restarts.
///
/// # Persistence
///
/// TODO: Implement persistence to save/restore event numbers across restarts.
/// Currently, event numbers reset to 1 on each device boot, which violates
/// the Matter specification. For production use, the last event number
/// should be persisted to non-volatile storage.
///
/// # Thread Safety
///
/// Uses atomic operations for lock-free event number generation.
#[derive(Debug)]
pub struct EventNumberGenerator {
    next: AtomicU64,
}

impl EventNumberGenerator {
    /// Create a new event number generator starting at 1.
    pub const fn new() -> Self {
        Self {
            next: AtomicU64::new(1),
        }
    }

    /// Create a new event number generator starting at the given value.
    ///
    /// Use this when restoring from persisted state.
    pub const fn with_start(start: u64) -> Self {
        Self {
            next: AtomicU64::new(start),
        }
    }

    /// Get the next event number.
    ///
    /// This atomically increments and returns the current value.
    /// The returned number is guaranteed to be unique and monotonically
    /// increasing within this generator's lifetime.
    pub fn next(&self) -> u64 {
        self.next.fetch_add(1, Ordering::SeqCst)
    }

    /// Get the current event number without incrementing.
    ///
    /// Useful for persistence - save this value and restore with [`with_start`].
    pub fn current(&self) -> u64 {
        self.next.load(Ordering::SeqCst)
    }

    /// Set the next event number.
    ///
    /// Use this when restoring from persisted state. The next call to
    /// [`next`] will return this value.
    ///
    /// # Safety
    ///
    /// Callers must ensure the new value is greater than any previously
    /// generated event number to maintain monotonicity.
    pub fn set(&self, value: u64) {
        self.next.store(value, Ordering::SeqCst);
    }
}

impl Default for EventNumberGenerator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_number_generator() {
        let gen = EventNumberGenerator::new();
        assert_eq!(gen.next(), 1);
        assert_eq!(gen.next(), 2);
        assert_eq!(gen.next(), 3);
        assert_eq!(gen.current(), 4);
    }

    #[test]
    fn test_event_number_generator_with_start() {
        let gen = EventNumberGenerator::with_start(100);
        assert_eq!(gen.next(), 100);
        assert_eq!(gen.next(), 101);
    }

    #[test]
    fn test_pending_event_new() {
        let event = PendingEvent::new(1, 0x003B, 0x01, 42, EventPriority::Info, 123456);
        assert_eq!(event.endpoint_id, 1);
        assert_eq!(event.cluster_id, 0x003B);
        assert_eq!(event.event_id, 0x01);
        assert_eq!(event.event_number, 42);
        assert_eq!(event.priority, EventPriority::Info);
        assert_eq!(event.system_timestamp_ms, 123456);
        assert!(event.payload.is_empty());
    }

    #[test]
    fn test_pending_event_with_payload() {
        let payload = [0x15, 0x24, 0x00, 0x01, 0x18];
        let event = PendingEvent::with_payload(
            1,
            0x003B,
            0x01,
            42,
            EventPriority::Info,
            123456,
            &payload,
        );
        assert_eq!(event.payload.as_slice(), &payload);
    }
}
