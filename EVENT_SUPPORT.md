# Matter Event Support for rs-matter

This document describes the implementation of Matter event reporting in rs-matter, enabling clusters like GenericSwitch to emit events that reach Matter controllers (e.g., Home Assistant).

## Status

**Fully Implemented** - Event support fully implemented per [Issue #36](https://github.com/project-chip/rs-matter/issues/36).

The rs-matter event support is complete including ReportDataResponder integration. Events are now automatically included in subscription reports when handlers implement `EventSource`.

To use events in a cluster handler:
1. Implement `EventSource` trait on your handler
2. Override `as_event_source()` in your `AsyncHandler` impl to return `Some(self)`
3. Queue events using `PendingEvent` and the encoding helpers in `generic_switch.rs`
4. Call `subscriptions.notify_event()` when events are queued
5. Events will be automatically collected and sent in ReportData responses

### Implementation Progress

- [x] EVENT_SUPPORT.md documentation
- [x] `rs-matter/src/im/event.rs` - TLV structures
- [x] `rs-matter/src/im/attr.rs` - Fix ReportDataRespTag
- [x] `rs-matter/src/im/mod.rs` - Export event module
- [x] `rs-matter/src/dm/types/event.rs` - EventSource trait
- [x] `rs-matter/src/dm/types/mod.rs` - Export event module
- [x] `rs-matter/src/dm/types/handler.rs` - Add as_event_source()
- [x] `rs-matter/src/dm/subscriptions.rs` - Add notify_event()
- [x] `rs-matter/src/dm.rs` - ReportDataResponder events (TODO scaffolding)
- [x] `rs-matter/src/dm/clusters/generic_switch.rs` - Event constants
- [x] Build and test (2026-01-14: cargo build + cargo test passed, 39 tests passed)

### ReportDataResponder Integration (Phase 2)

- [x] `rs-matter/src/im/attr.rs` - Add event_requests() to ReportDataReq
- [x] `rs-matter/src/dm/types/reply.rs` - Add collect_pending_events() to HandlerInvoker
- [x] `rs-matter/src/dm/types/handler.rs` - Add EventCollector trait
- [x] `rs-matter/src/dm.rs` - Modify respond() to collect events
- [x] `rs-matter/src/dm.rs` - Modify send() signature
- [x] `rs-matter/src/dm.rs` - Write EventReports in end_reply()
- [x] Build and test (2026-01-14: cargo build + cargo test -p rs-matter passed, 39 tests passed)

## Background

Matter events are distinct from attributes:
- **Attributes**: State that can be read/subscribed (e.g., temperature = 22.5°C)
- **Events**: Occurrences that happened at a point in time (e.g., button pressed)

Events are critical for:
- GenericSwitch (button presses)
- Alarms and alerts
- State change notifications
- Diagnostic information

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                            Event Flow                                        │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  1. Cluster handler detects event (e.g., button press)                      │
│     └─> Calls EventSource::queue_event()                                    │
│         └─> Event stored in handler's internal queue                        │
│                                                                              │
│  2. Handler notifies subscription system                                     │
│     └─> subscriptions.notify_event(endpoint, cluster, event_id)             │
│         └─> Marks relevant subscriptions as "changed"                       │
│                                                                              │
│  3. Subscription timer fires or change detected                              │
│     └─> ReportDataResponder::respond() called                               │
│         └─> Iterates handlers, calls as_event_source()                      │
│             └─> For each EventSource, calls take_pending_events()           │
│                 └─> Events encoded as EventReportIB in TLV                  │
│                                                                              │
│  4. ReportData message sent to controller                                    │
│     └─> Contains EventReports array (tag 2)                                 │
│         └─> Controller processes events (e.g., HA triggers automation)      │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

## Implementation Plan

### Files to Create

| File | Purpose |
|------|---------|
| `rs-matter/src/im/event.rs` | EventDataIB, EventReportIB, EventStatusIB TLV structures |
| `rs-matter/src/dm/types/event.rs` | EventSource trait, PendingEvent, EventNumberGenerator |
| `rs-matter/src/dm/clusters/generic_switch.rs` | GenericSwitch cluster event constants and helpers |

### Files to Modify

| File | Change |
|------|--------|
| `rs-matter/src/im/mod.rs` | Export event module |
| `rs-matter/src/im/attr.rs` | Fix `_EventReport` → `EventReports` in ReportDataRespTag |
| `rs-matter/src/dm/types/mod.rs` | Export event module |
| `rs-matter/src/dm/types/handler.rs` | Add `as_event_source()` to AsyncHandler trait |
| `rs-matter/src/dm/subscriptions.rs` | Add `notify_event()` method |
| `rs-matter/src/dm.rs` | Add event iteration in ReportDataResponder |

---

## TLV Structures (Matter Spec 10.6)

### EventPriority

```rust
/// Event priority levels per Matter Core Specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum EventPriority {
    /// Debug priority - may be discarded
    Debug = 0,
    /// Info priority - normal events
    Info = 1,
    /// Critical priority - must be delivered
    Critical = 2,
}
```

### EventDataIB

Per Matter spec, EventDataIB contains:

| Tag | Field | Type | Required |
|-----|-------|------|----------|
| 0 | path | EventPathIB | Yes |
| 1 | event_number | u64 | Yes |
| 2 | priority | u8 | Yes |
| 3 | epoch_timestamp | u64 | One of 3-6 |
| 4 | system_timestamp | u64 | One of 3-6 |
| 5 | delta_epoch_timestamp | u64 | One of 3-6 |
| 6 | delta_system_timestamp | u64 | One of 3-6 |
| 7 | data | ANY | No |

```rust
pub struct EventDataIB<'a> {
    pub path: EventPath,
    pub event_number: u64,
    pub priority: EventPriority,
    pub epoch_timestamp: Option<u64>,
    pub system_timestamp: Option<u64>,
    pub delta_epoch_timestamp: Option<u64>,  // TODO: Implement
    pub delta_system_timestamp: Option<u64>, // TODO: Implement
    pub data: Option<&'a [u8]>,              // Pre-encoded TLV payload
}
```

### EventReportIB

```rust
pub struct EventReportIB<'a> {
    pub event_status: Option<EventStatusIB>,  // Tag 0 - for errors
    pub event_data: Option<EventDataIB<'a>>,  // Tag 1 - actual event
}
```

### EventStatusIB

```rust
pub struct EventStatusIB {
    pub path: EventPath,   // Tag 0
    pub status: StatusIB,  // Tag 1
}
```

---

## EventSource Trait

Cluster handlers that emit events implement this trait:

```rust
/// A pending event ready to be reported to subscribers.
pub struct PendingEvent {
    pub endpoint_id: EndptId,
    pub cluster_id: ClusterId,
    pub event_id: u32,
    pub event_number: u64,
    pub priority: EventPriority,
    pub system_timestamp_ms: u64,
    pub payload: heapless::Vec<u8, 64>,  // Pre-encoded TLV
}

/// Trait for cluster handlers that can produce Matter events.
pub trait EventSource: Send + Sync {
    /// Returns pending events and clears the internal queue.
    fn take_pending_events(&self) -> heapless::Vec<PendingEvent, 16>;

    /// Check if there are pending events without draining.
    fn has_pending_events(&self) -> bool;

    /// Filter events by minimum event number.
    /// TODO: Implement for EventFilter support
    fn take_events_since(&self, min_event_number: u64) -> heapless::Vec<PendingEvent, 16> {
        self.take_pending_events()
    }
}
```

### Usage Example

```rust
impl EventSource for MyClusterHandler {
    fn take_pending_events(&self) -> heapless::Vec<PendingEvent, 16> {
        let mut events = heapless::Vec::new();

        // Drain internal queue
        while let Some(event) = self.event_queue.lock().pop() {
            let _ = events.push(event);
        }

        events
    }

    fn has_pending_events(&self) -> bool {
        !self.event_queue.lock().is_empty()
    }
}
```

---

## GenericSwitch Events (Cluster 0x003B)

### Event IDs

| ID | Name | Description | Payload |
|----|------|-------------|---------|
| 0x00 | SwitchLatched | Position changed (latching) | `{ NewPosition [0]: u8 }` |
| 0x01 | InitialPress | Button pressed down | `{ NewPosition [0]: u8 }` |
| 0x02 | LongPress | Held past threshold | `{ NewPosition [0]: u8 }` |
| 0x03 | ShortRelease | Released after short press | `{ PreviousPosition [0]: u8 }` |
| 0x04 | LongRelease | Released after long press | `{ PreviousPosition [0]: u8 }` |
| 0x05 | MultiPressOngoing | Multi-press in progress | `{ NewPosition [0]: u8, CurrentCount [1]: u8 }` |
| 0x06 | MultiPressComplete | Multi-press finished | `{ PreviousPosition [0]: u8, TotalCount [1]: u8 }` |

### Typical Button Press Sequence

**Single press:**
1. `InitialPress { NewPosition: 1 }` - Button down
2. `ShortRelease { PreviousPosition: 1 }` - Button up

**Double press:**
1. `InitialPress { NewPosition: 1 }`
2. `ShortRelease { PreviousPosition: 1 }`
3. `InitialPress { NewPosition: 1 }`
4. `MultiPressComplete { PreviousPosition: 1, TotalCount: 2 }`

**Long press:**
1. `InitialPress { NewPosition: 1 }`
2. `LongPress { NewPosition: 1 }` - After threshold
3. `LongRelease { PreviousPosition: 1 }`

---

## AsyncHandler Integration

Add to the `AsyncHandler` trait:

```rust
/// Returns this handler as an EventSource if it supports events.
///
/// Override this method in handlers that implement EventSource
/// to enable event reporting for their cluster.
fn as_event_source(&self) -> Option<&dyn EventSource> {
    None  // Default: no event support
}
```

Handlers that emit events override this:

```rust
impl AsyncHandler for GenericSwitchHandler {
    // ... other methods ...

    fn as_event_source(&self) -> Option<&dyn EventSource> {
        Some(self)
    }
}
```

---

## Subscription Integration

### notify_event()

Add to `Subscriptions` trait:

```rust
/// Notify that an event occurred on a cluster.
///
/// This marks relevant subscriptions as changed, triggering
/// a report on the next interval.
///
/// TODO: Support urgent events that bypass min_interval
fn notify_event(&self, endpoint_id: EndptId, cluster_id: ClusterId, event_id: u32);
```

### ReportDataResponder Changes

After attribute reports in `respond()`:

```rust
// === EVENT REPORTS ===
let event_requests = req.event_requests()?;

if event_requests.is_some() {
    let mut events_written = false;

    // Iterate endpoints and collect events from EventSource handlers
    for endpoint in self.node.endpoints() {
        for cluster in endpoint.clusters() {
            if let Some(handler) = self.get_handler(endpoint.id, cluster.id) {
                if let Some(event_source) = handler.as_event_source() {
                    if event_source.has_pending_events() {
                        if !events_written {
                            tw.start_array(&TLVTag::Context(ReportDataRespTag::EventReports as u8))?;
                            events_written = true;
                        }

                        for event in event_source.take_pending_events() {
                            let report = EventReportIB { /* ... */ };
                            report.to_tlv(&TLVTag::Anonymous, tw)?;
                        }
                    }
                }
            }
        }
    }

    if events_written {
        tw.end_container()?;
    }
}
```

---

## TODOs (Future Work)

These features are scaffolded but not fully implemented:

1. **Epoch timestamps** - Currently only `system_timestamp` is used
2. **Delta timestamps** - For bandwidth optimization in subscriptions
3. **Event filtering** - `EventFilter` support (min event number)
4. **Event path wildcards** - Proper wildcard expansion for event subscriptions
5. **Urgent events** - Bypass `min_interval` for critical events
6. **Event persistence** - Save/restore event numbers across restarts
7. **Long press detection** - Timing-based long press threshold
8. **Multi-press timing** - Detect double/triple press sequences
9. **Latching switch** - `SwitchLatched` event for non-momentary switches
10. **FromTLV for events** - Parse events received from other Matter nodes
11. **Event chunking** - Handle large event payloads across multiple reports

---

## Testing

### Build

```bash
cargo build --all-features
cargo test
```

### Manual Testing

1. Implement `EventSource` in a cluster handler (e.g., GenericSwitch)
2. Commission device to Home Assistant
3. Trigger event (e.g., press button)
4. Verify event appears in Home Assistant

### Expected Log Output

```
[Matter] GenericSwitch endpoint 5: InitialPress event queued (event_number=1)
[Matter] GenericSwitch endpoint 5: ShortRelease event queued (event_number=2)
[Matter] Subscription report: 2 events included
```

---

## References

- [Matter Core Specification 1.4, Section 10.6 - Event Interaction](https://csa-iot.org/developer-resource/specifications-download-request/)
- [Matter Application Cluster Specification - GenericSwitch (0x003B)](https://csa-iot.org/developer-resource/specifications-download-request/)
- [rs-matter Issue #36 - Event Support](https://github.com/project-chip/rs-matter/issues/36)
