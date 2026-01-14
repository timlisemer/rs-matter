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

/// Event timestamp representation for delta calculations.
///
/// Matter events can use either epoch timestamps (microseconds since Unix epoch)
/// or system timestamps (milliseconds since boot). Delta timestamps encode the
/// difference from the previous event's timestamp, saving bandwidth.
///
/// Per Matter spec:
/// - First event in a report uses absolute timestamp
/// - Subsequent events use delta timestamps relative to the previous event
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventTimestamp {
    /// Epoch timestamp in microseconds since Unix epoch (Jan 1, 1970).
    Epoch(u64),
    /// System timestamp in milliseconds since device boot.
    System(u64),
}

impl EventTimestamp {
    /// Create an EventTimestamp from a PendingEvent.
    ///
    /// Prefers epoch timestamp if available, otherwise uses system timestamp.
    pub fn from_event(event: &PendingEvent) -> Self {
        if let Some(epoch_us) = event.epoch_timestamp_us {
            Self::Epoch(epoch_us)
        } else {
            Self::System(event.system_timestamp_ms)
        }
    }

    /// Calculate the delta from this timestamp to another.
    ///
    /// Returns `Some((delta_epoch, delta_system))` if both timestamps are of the same type.
    /// Returns `None` if timestamp types are incompatible (caller should use absolute).
    ///
    /// - For Epoch timestamps: returns `(Some(delta_us), None)`
    /// - For System timestamps: returns `(None, Some(delta_ms))`
    pub fn delta_to(&self, other: &Self) -> Option<(Option<u64>, Option<u64>)> {
        match (self, other) {
            (Self::Epoch(prev), Self::Epoch(curr)) => {
                // Delta is current - previous (saturating to 0 if somehow negative)
                let delta = curr.saturating_sub(*prev);
                Some((Some(delta), None))
            }
            (Self::System(prev), Self::System(curr)) => {
                let delta = curr.saturating_sub(*prev);
                Some((None, Some(delta)))
            }
            // Mixed types - fall back to absolute timestamps
            _ => None,
        }
    }

    /// Check if this timestamp uses epoch time.
    pub fn is_epoch(&self) -> bool {
        matches!(self, Self::Epoch(_))
    }
}

/// Sort events by priority: Critical first, then Info, then Debug.
///
/// This ensures that important events are sent first when chunking is needed.
/// Events of the same priority maintain their original order (stable sort).
///
/// # Arguments
/// - `events`: The events to sort in place
pub fn sort_events_by_priority(events: &mut heapless::Vec<PendingEvent, MAX_PENDING_EVENTS>) {
    // Simple insertion sort for small arrays (heapless Vec doesn't have sort_by)
    // Sort order: Critical (0) < Info (1) < Debug (2)
    let len = events.len();
    for i in 1..len {
        let mut j = i;
        while j > 0 {
            let priority_j = event_priority_order(&events[j].priority);
            let priority_j_minus_1 = event_priority_order(&events[j - 1].priority);
            if priority_j < priority_j_minus_1 {
                events.swap(j, j - 1);
                j -= 1;
            } else {
                break;
            }
        }
    }
}

/// Get the sort order for an event priority.
/// Lower values sort first (Critical = 0, Info = 1, Debug = 2).
fn event_priority_order(priority: &crate::im::EventPriority) -> u8 {
    match priority {
        crate::im::EventPriority::Critical => 0,
        crate::im::EventPriority::Info => 1,
        crate::im::EventPriority::Debug => 2,
    }
}

/// Check if an event matches an event path (with wildcard support).
///
/// Per Matter spec, `None` in any path component acts as a wildcard that matches everything.
/// This is used to filter collected events against the requested event paths.
///
/// # Arguments
/// - `event`: The pending event to check
/// - `endpoint`: The endpoint filter (None = match all)
/// - `cluster`: The cluster filter (None = match all)
/// - `event_id`: The event ID filter (None = match all)
///
/// # Returns
/// `true` if the event matches all specified path components
pub fn event_matches_path(
    event: &PendingEvent,
    endpoint: Option<EndptId>,
    cluster: Option<ClusterId>,
    event_id: Option<u32>,
) -> bool {
    // None acts as wildcard - matches everything
    let endpoint_match = endpoint.is_none() || endpoint == Some(event.endpoint_id);
    let cluster_match = cluster.is_none() || cluster == Some(event.cluster_id);
    let event_match = event_id.is_none() || event_id == Some(event.event_id);

    endpoint_match && cluster_match && event_match
}

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
    /// Used when epoch timestamp is not available.
    pub system_timestamp_ms: u64,
    /// Epoch timestamp in microseconds since Unix epoch (Jan 1, 1970).
    ///
    /// Preferred over system timestamp when the device has reliable time.
    /// Set to `Some` when epoch time is available, `None` to use system timestamp.
    pub epoch_timestamp_us: Option<u64>,
    /// Pre-encoded TLV payload with cluster-specific event data.
    ///
    /// The payload format is defined by the cluster specification.
    /// For example, GenericSwitch InitialPress: `{ NewPosition [0]: u8 }`.
    #[cfg_attr(feature = "defmt", defmt(Debug2Format))]
    pub payload: heapless::Vec<u8, MAX_EVENT_PAYLOAD_SIZE>,
}

impl PendingEvent {
    /// Create a new pending event with system timestamp (milliseconds since boot).
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
            epoch_timestamp_us: None,
            payload: heapless::Vec::new(),
        }
    }

    /// Create a new pending event with epoch timestamp (microseconds since Unix epoch).
    ///
    /// Use this when the device has reliable time synchronization.
    /// The epoch timestamp is preferred by Matter controllers.
    pub fn new_with_epoch(
        endpoint_id: EndptId,
        cluster_id: ClusterId,
        event_id: u32,
        event_number: u64,
        priority: EventPriority,
        epoch_timestamp_us: u64,
    ) -> Self {
        Self {
            endpoint_id,
            cluster_id,
            event_id,
            event_number,
            priority,
            system_timestamp_ms: 0, // Not used when epoch is set
            epoch_timestamp_us: Some(epoch_timestamp_us),
            payload: heapless::Vec::new(),
        }
    }

    /// Create a new pending event with payload and system timestamp.
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

    /// Create a new pending event with payload and epoch timestamp.
    ///
    /// Use this when the device has reliable time synchronization.
    pub fn with_payload_and_epoch(
        endpoint_id: EndptId,
        cluster_id: ClusterId,
        event_id: u32,
        event_number: u64,
        priority: EventPriority,
        epoch_timestamp_us: u64,
        payload: &[u8],
    ) -> Self {
        let mut event = Self::new_with_epoch(
            endpoint_id,
            cluster_id,
            event_id,
            event_number,
            priority,
            epoch_timestamp_us,
        );
        // Truncate if payload is too large
        let len = payload.len().min(MAX_EVENT_PAYLOAD_SIZE);
        event.payload.extend_from_slice(&payload[..len]).ok();
        event
    }

    /// Set the epoch timestamp on an existing event.
    ///
    /// This is useful when you want to add epoch timestamp to an event
    /// that was created with system timestamp.
    pub fn set_epoch_timestamp(&mut self, epoch_timestamp_us: u64) {
        self.epoch_timestamp_us = Some(epoch_timestamp_us);
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
    /// The default implementation takes all pending events and filters them.
    /// Implementations may override this for more efficient filtering.
    fn take_events_since(
        &self,
        min_event_number: u64,
    ) -> heapless::Vec<PendingEvent, MAX_PENDING_EVENTS> {
        let all_events = self.take_pending_events();
        let mut filtered = heapless::Vec::new();
        for event in all_events {
            if event.event_number >= min_event_number {
                // Ignore push failures if queue is full
                filtered.push(event).ok();
            }
        }
        filtered
    }
}

/// Trait for collecting events from handler chains.
///
/// This trait enables recursive event collection from chained handlers.
/// Implement this on handler wrapper types (like `ChainedHandler`) to
/// collect events from all handlers in the chain.
///
/// # Usage
///
/// The data model calls `collect_events()` during subscription report
/// generation to gather all pending events from the handler hierarchy.
pub trait EventCollector {
    /// Collect pending events from this handler and any nested handlers.
    ///
    /// Events are appended to the provided vector. Implementations should
    /// call `collect_events` recursively on nested handlers.
    fn collect_events(&self, events: &mut heapless::Vec<PendingEvent, { MAX_PENDING_EVENTS }>);
}

/// Trait for persisting event numbers across device restarts.
///
/// Per Matter specification, event numbers must never decrease, even across
/// device restarts. Implementations store event numbers to non-volatile storage.
///
/// # Wear Leveling
///
/// To avoid excessive flash wear, implementations should NOT persist on every
/// event. Instead, use batching - persist every N events. On restore, add a
/// safety margin (e.g., the batch size) to account for events that occurred
/// after the last persist but before the restart.
///
/// # Platform Examples
///
/// - **ESP32**: Use NVS (`esp_idf_svc::nvs`)
/// - **nRF/RP2040**: Use `embedded-storage` trait
/// - **std**: Use file-based persistence
pub trait EventNumberPersistence: Send + Sync {
    /// Load the last persisted event number.
    ///
    /// Returns `Ok(None)` if no event number has been persisted yet (first boot).
    /// Returns `Err` on storage read failure.
    fn load(&self) -> Result<Option<u64>, crate::error::Error>;

    /// Store the current event number.
    ///
    /// Called periodically (every `batch_size` events) to persist the current
    /// event number. Also called on graceful shutdown via `persist_now()`.
    fn store(&self, value: u64) -> Result<(), crate::error::Error>;
}

/// A no-op persistence implementation.
///
/// Event numbers are not persisted - they reset to 1 on each device boot.
/// This violates the Matter spec but is useful for development/testing.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopPersistence;

impl EventNumberPersistence for NoopPersistence {
    fn load(&self) -> Result<Option<u64>, crate::error::Error> {
        Ok(None)
    }

    fn store(&self, _value: u64) -> Result<(), crate::error::Error> {
        Ok(())
    }
}

/// Default batch size for event number persistence.
///
/// Persisting every 100 events balances flash wear vs. potential event number
/// gaps on unexpected restart.
pub const DEFAULT_PERSIST_BATCH_SIZE: u64 = 100;

/// Global event number generator with optional persistence.
///
/// Provides monotonically increasing event numbers across all clusters.
/// Per Matter spec, event numbers must never decrease, even across restarts.
///
/// # Persistence
///
/// Use [`EventNumberGeneratorPersistent`] for production to persist event numbers
/// across restarts. The basic `EventNumberGenerator` does not persist and is
/// suitable for development/testing only.
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

/// Event number generator with automatic persistence support.
///
/// This generator automatically persists event numbers to non-volatile storage
/// every `batch_size` events to comply with Matter specification requirements.
///
/// # Wear Leveling
///
/// To minimize flash wear, event numbers are not persisted on every call to
/// `next()`. Instead, they are persisted every `batch_size` events (default: 100).
/// On restore, a safety margin is added to account for events between the last
/// persist and an unexpected restart.
///
/// # Usage
///
/// ```ignore
/// use rs_matter::dm::types::event::{EventNumberGeneratorPersistent, EventNumberPersistence};
///
/// struct MyPersistence { /* NVS/file handle */ }
/// impl EventNumberPersistence for MyPersistence {
///     fn load(&self) -> Result<Option<u64>, Error> { /* load from storage */ }
///     fn store(&self, value: u64) -> Result<(), Error> { /* write to storage */ }
/// }
///
/// let persistence = MyPersistence { /* ... */ };
/// let gen = EventNumberGeneratorPersistent::new(persistence)?;
///
/// // On graceful shutdown:
/// gen.persist_now()?;
/// ```
pub struct EventNumberGeneratorPersistent<P: EventNumberPersistence> {
    next: AtomicU64,
    last_persisted: AtomicU64,
    batch_size: u64,
    persistence: P,
}

impl<P: EventNumberPersistence> EventNumberGeneratorPersistent<P> {
    /// Create a new persistent event number generator.
    ///
    /// Loads the last persisted event number (if any) and adds a safety margin.
    /// Returns an error if loading from storage fails.
    pub fn new(persistence: P) -> Result<Self, crate::error::Error> {
        Self::with_batch_size(persistence, DEFAULT_PERSIST_BATCH_SIZE)
    }

    /// Create a new persistent event number generator with custom batch size.
    ///
    /// The `batch_size` determines how often event numbers are persisted.
    /// Smaller values provide better recovery but cause more flash wear.
    pub fn with_batch_size(persistence: P, batch_size: u64) -> Result<Self, crate::error::Error> {
        let loaded = persistence.load()?;

        // Add safety margin to account for events after last persist but before restart
        let start = loaded
            .map(|n| n + batch_size)
            .unwrap_or(1);

        Ok(Self {
            next: AtomicU64::new(start),
            last_persisted: AtomicU64::new(loaded.unwrap_or(0)),
            batch_size,
            persistence,
        })
    }

    /// Get the next event number.
    ///
    /// This atomically increments and returns the current value.
    /// Automatically persists when the batch threshold is reached.
    ///
    /// Note: Persistence errors are silently ignored. Use `persist_now()` for
    /// guaranteed persistence on graceful shutdown.
    pub fn next(&self) -> u64 {
        let current = self.next.fetch_add(1, Ordering::SeqCst);

        // Check if we need to persist
        let last = self.last_persisted.load(Ordering::SeqCst);
        if current >= last + self.batch_size {
            // Attempt to update last_persisted atomically
            if self
                .last_persisted
                .compare_exchange(last, current, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                // We won the race, persist the value
                // Ignore errors - persistence is best-effort during normal operation
                let _ = self.persistence.store(current);
            }
        }

        current
    }

    /// Get the current event number without incrementing.
    pub fn current(&self) -> u64 {
        self.next.load(Ordering::SeqCst)
    }

    /// Persist the current event number immediately.
    ///
    /// Call this on graceful shutdown to minimize event number gaps
    /// on the next restart.
    pub fn persist_now(&self) -> Result<(), crate::error::Error> {
        let current = self.current();
        self.persistence.store(current)?;
        self.last_persisted.store(current, Ordering::SeqCst);
        Ok(())
    }

    /// Get a reference to the persistence backend.
    pub fn persistence(&self) -> &P {
        &self.persistence
    }
}

impl<P: EventNumberPersistence + core::fmt::Debug> core::fmt::Debug
    for EventNumberGeneratorPersistent<P>
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("EventNumberGeneratorPersistent")
            .field("next", &self.next.load(Ordering::SeqCst))
            .field("last_persisted", &self.last_persisted.load(Ordering::SeqCst))
            .field("batch_size", &self.batch_size)
            .field("persistence", &self.persistence)
            .finish()
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
        assert!(event.epoch_timestamp_us.is_none());
        assert!(event.payload.is_empty());
    }

    #[test]
    fn test_pending_event_new_with_epoch() {
        let event =
            PendingEvent::new_with_epoch(1, 0x003B, 0x01, 42, EventPriority::Info, 1700000000000000);
        assert_eq!(event.endpoint_id, 1);
        assert_eq!(event.cluster_id, 0x003B);
        assert_eq!(event.event_number, 42);
        assert_eq!(event.epoch_timestamp_us, Some(1700000000000000));
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
        assert!(event.epoch_timestamp_us.is_none());
    }

    #[test]
    fn test_pending_event_with_payload_and_epoch() {
        let payload = [0x15, 0x24, 0x00, 0x01, 0x18];
        let event = PendingEvent::with_payload_and_epoch(
            1,
            0x003B,
            0x01,
            42,
            EventPriority::Info,
            1700000000000000,
            &payload,
        );
        assert_eq!(event.payload.as_slice(), &payload);
        assert_eq!(event.epoch_timestamp_us, Some(1700000000000000));
    }

    #[test]
    fn test_pending_event_set_epoch() {
        let mut event = PendingEvent::new(1, 0x003B, 0x01, 42, EventPriority::Info, 123456);
        assert!(event.epoch_timestamp_us.is_none());
        event.set_epoch_timestamp(1700000000000000);
        assert_eq!(event.epoch_timestamp_us, Some(1700000000000000));
    }

    // Test for event filtering by event number
    struct MockEventSource {
        events: core::cell::RefCell<heapless::Vec<PendingEvent, MAX_PENDING_EVENTS>>,
    }

    impl EventSource for MockEventSource {
        fn take_pending_events(&self) -> heapless::Vec<PendingEvent, MAX_PENDING_EVENTS> {
            core::mem::take(&mut *self.events.borrow_mut())
        }

        fn has_pending_events(&self) -> bool {
            !self.events.borrow().is_empty()
        }
    }

    #[test]
    fn test_take_events_since_filters_by_event_number() {
        let source = MockEventSource {
            events: core::cell::RefCell::new(heapless::Vec::new()),
        };

        // Add events with different event numbers
        source
            .events
            .borrow_mut()
            .push(PendingEvent::new(1, 0x003B, 0x01, 5, EventPriority::Info, 100))
            .ok();
        source
            .events
            .borrow_mut()
            .push(PendingEvent::new(1, 0x003B, 0x01, 10, EventPriority::Info, 200))
            .ok();
        source
            .events
            .borrow_mut()
            .push(PendingEvent::new(1, 0x003B, 0x01, 15, EventPriority::Info, 300))
            .ok();

        // Filter events with event_number >= 10
        let filtered = source.take_events_since(10);

        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0].event_number, 10);
        assert_eq!(filtered[1].event_number, 15);
    }

    #[test]
    fn test_take_events_since_returns_all_when_min_is_zero() {
        let source = MockEventSource {
            events: core::cell::RefCell::new(heapless::Vec::new()),
        };

        source
            .events
            .borrow_mut()
            .push(PendingEvent::new(1, 0x003B, 0x01, 1, EventPriority::Info, 100))
            .ok();
        source
            .events
            .borrow_mut()
            .push(PendingEvent::new(1, 0x003B, 0x01, 2, EventPriority::Info, 200))
            .ok();

        let filtered = source.take_events_since(0);

        assert_eq!(filtered.len(), 2);
    }

    // Mock persistence for testing
    struct MockPersistence {
        value: core::cell::RefCell<Option<u64>>,
        store_count: core::cell::RefCell<usize>,
    }

    impl MockPersistence {
        fn new() -> Self {
            Self {
                value: core::cell::RefCell::new(None),
                store_count: core::cell::RefCell::new(0),
            }
        }

        fn with_value(value: u64) -> Self {
            Self {
                value: core::cell::RefCell::new(Some(value)),
                store_count: core::cell::RefCell::new(0),
            }
        }
    }

    impl EventNumberPersistence for MockPersistence {
        fn load(&self) -> Result<Option<u64>, crate::error::Error> {
            Ok(*self.value.borrow())
        }

        fn store(&self, value: u64) -> Result<(), crate::error::Error> {
            *self.value.borrow_mut() = Some(value);
            *self.store_count.borrow_mut() += 1;
            Ok(())
        }
    }

    #[test]
    fn test_persistent_generator_first_boot() {
        let persistence = MockPersistence::new();
        let gen = EventNumberGeneratorPersistent::new(persistence).unwrap();

        // First boot starts at 1
        assert_eq!(gen.next(), 1);
        assert_eq!(gen.next(), 2);
        assert_eq!(gen.next(), 3);
    }

    #[test]
    fn test_persistent_generator_restore() {
        let persistence = MockPersistence::with_value(50);
        let gen = EventNumberGeneratorPersistent::new(persistence).unwrap();

        // After restore, starts at persisted + batch_size (50 + 100 = 150)
        assert_eq!(gen.current(), 150);
        assert_eq!(gen.next(), 150);
        assert_eq!(gen.next(), 151);
    }

    #[test]
    fn test_persistent_generator_batching() {
        let persistence = MockPersistence::new();
        let gen =
            EventNumberGeneratorPersistent::with_batch_size(persistence, 10).unwrap();

        // Generate 25 events
        for _ in 0..25 {
            gen.next();
        }

        // Should have persisted at 10 and 20
        assert_eq!(*gen.persistence().store_count.borrow(), 2);
    }

    #[test]
    fn test_persistent_generator_persist_now() {
        let persistence = MockPersistence::new();
        let gen =
            EventNumberGeneratorPersistent::with_batch_size(persistence, 100).unwrap();

        // Generate a few events (not enough to trigger auto-persist)
        gen.next();
        gen.next();
        gen.next();

        assert_eq!(*gen.persistence().store_count.borrow(), 0);

        // Force persist
        gen.persist_now().unwrap();

        assert_eq!(*gen.persistence().store_count.borrow(), 1);
        assert_eq!(*gen.persistence().value.borrow(), Some(4));
    }

    #[test]
    fn test_noop_persistence() {
        let persistence = NoopPersistence;

        // Load returns None
        assert_eq!(persistence.load().unwrap(), None);

        // Store succeeds but doesn't do anything
        assert!(persistence.store(100).is_ok());
        assert_eq!(persistence.load().unwrap(), None);
    }

    // Tests for EventTimestamp

    #[test]
    fn test_event_timestamp_from_event_epoch() {
        let event = PendingEvent::new_with_epoch(1, 0x003B, 0x01, 1, EventPriority::Info, 1000000);
        let ts = EventTimestamp::from_event(&event);
        assert_eq!(ts, EventTimestamp::Epoch(1000000));
        assert!(ts.is_epoch());
    }

    #[test]
    fn test_event_timestamp_from_event_system() {
        let event = PendingEvent::new(1, 0x003B, 0x01, 1, EventPriority::Info, 5000);
        let ts = EventTimestamp::from_event(&event);
        assert_eq!(ts, EventTimestamp::System(5000));
        assert!(!ts.is_epoch());
    }

    #[test]
    fn test_event_timestamp_delta_epoch() {
        let ts1 = EventTimestamp::Epoch(1000000);
        let ts2 = EventTimestamp::Epoch(1500000);

        let result = ts1.delta_to(&ts2);
        assert_eq!(result, Some((Some(500000), None)));
    }

    #[test]
    fn test_event_timestamp_delta_system() {
        let ts1 = EventTimestamp::System(1000);
        let ts2 = EventTimestamp::System(1500);

        let result = ts1.delta_to(&ts2);
        assert_eq!(result, Some((None, Some(500))));
    }

    #[test]
    fn test_event_timestamp_delta_mixed_returns_none() {
        let ts1 = EventTimestamp::Epoch(1000000);
        let ts2 = EventTimestamp::System(5000);

        let result = ts1.delta_to(&ts2);
        assert_eq!(result, None);
    }

    #[test]
    fn test_event_timestamp_delta_saturating() {
        // If current is somehow less than previous, saturating_sub returns 0
        let ts1 = EventTimestamp::System(5000);
        let ts2 = EventTimestamp::System(1000);

        let result = ts1.delta_to(&ts2);
        assert_eq!(result, Some((None, Some(0))));
    }

    // Tests for event_matches_path

    #[test]
    fn test_event_matches_path_exact() {
        let event = PendingEvent::new(1, 0x003B, 0x01, 1, EventPriority::Info, 1000);

        // Exact match
        assert!(super::event_matches_path(&event, Some(1), Some(0x003B), Some(0x01)));

        // Wrong endpoint
        assert!(!super::event_matches_path(&event, Some(2), Some(0x003B), Some(0x01)));

        // Wrong cluster
        assert!(!super::event_matches_path(&event, Some(1), Some(0x003C), Some(0x01)));

        // Wrong event
        assert!(!super::event_matches_path(&event, Some(1), Some(0x003B), Some(0x02)));
    }

    #[test]
    fn test_event_matches_path_wildcards() {
        let event = PendingEvent::new(5, 0x0006, 0x00, 1, EventPriority::Info, 1000);

        // All wildcards (matches everything)
        assert!(super::event_matches_path(&event, None, None, None));

        // Endpoint wildcard
        assert!(super::event_matches_path(&event, None, Some(0x0006), Some(0x00)));

        // Cluster wildcard
        assert!(super::event_matches_path(&event, Some(5), None, Some(0x00)));

        // Event ID wildcard
        assert!(super::event_matches_path(&event, Some(5), Some(0x0006), None));

        // Multiple wildcards
        assert!(super::event_matches_path(&event, None, None, Some(0x00)));
        assert!(super::event_matches_path(&event, Some(5), None, None));
    }

    #[test]
    fn test_event_matches_path_partial_mismatch() {
        let event = PendingEvent::new(1, 0x003B, 0x01, 1, EventPriority::Info, 1000);

        // Endpoint matches, cluster wildcard, wrong event
        assert!(!super::event_matches_path(&event, Some(1), None, Some(0x02)));

        // Cluster matches, endpoint wildcard, wrong event
        assert!(!super::event_matches_path(&event, None, Some(0x003B), Some(0x99)));
    }

    // Tests for sort_events_by_priority

    #[test]
    fn test_sort_events_by_priority() {
        let mut events = heapless::Vec::new();

        // Add events in Debug, Info, Critical order
        events
            .push(PendingEvent::new(1, 0x003B, 0x01, 1, EventPriority::Debug, 1000))
            .ok();
        events
            .push(PendingEvent::new(1, 0x003B, 0x01, 2, EventPriority::Info, 2000))
            .ok();
        events
            .push(PendingEvent::new(1, 0x003B, 0x01, 3, EventPriority::Critical, 3000))
            .ok();
        events
            .push(PendingEvent::new(1, 0x003B, 0x01, 4, EventPriority::Info, 4000))
            .ok();

        super::sort_events_by_priority(&mut events);

        // Should be sorted: Critical first, then Info (2 events), then Debug
        assert_eq!(events[0].priority, EventPriority::Critical);
        assert_eq!(events[0].event_number, 3);
        assert_eq!(events[1].priority, EventPriority::Info);
        assert_eq!(events[2].priority, EventPriority::Info);
        assert_eq!(events[3].priority, EventPriority::Debug);
        assert_eq!(events[3].event_number, 1);
    }

    #[test]
    fn test_sort_events_empty() {
        let mut events: heapless::Vec<PendingEvent, 16> = heapless::Vec::new();
        super::sort_events_by_priority(&mut events);
        assert!(events.is_empty());
    }

    #[test]
    fn test_sort_events_single() {
        let mut events = heapless::Vec::new();
        events
            .push(PendingEvent::new(1, 0x003B, 0x01, 1, EventPriority::Info, 1000))
            .ok();
        super::sort_events_by_priority(&mut events);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].priority, EventPriority::Info);
    }
}
