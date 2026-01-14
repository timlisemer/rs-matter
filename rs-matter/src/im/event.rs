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

//! Matter Event TLV structures for the Interaction Model.
//!
//! This module provides TLV encoding/decoding for Matter events as specified
//! in the Matter Core Specification Section 10.6.
//!
//! # Overview
//!
//! Events are point-in-time occurrences that are reported to subscribers.
//! Unlike attributes (which represent current state), events represent
//! things that happened (e.g., button pressed, alarm triggered).
//!
//! # Key Types
//!
//! - [`EventPriority`] - Debug, Info, or Critical priority levels
//! - [`EventDataIB`] - The actual event data with path, number, timestamp, and payload
//! - [`EventReportIB`] - Wrapper containing either event data or error status
//! - [`EventStatusIB`] - Error status for failed event reads

use crate::error::Error;
use crate::tlv::{FromTLV, TLVElement, TLVTag, TLVWrite, ToTLV};

use super::{EventPath, Status};

/// Event priority levels per Matter Core Specification.
///
/// Priority determines event delivery guarantees and storage behavior:
/// - `Debug`: May be discarded under memory pressure
/// - `Info`: Normal priority, delivered in order
/// - `Critical`: Must be delivered, may bypass intervals
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[repr(u8)]
pub enum EventPriority {
    /// Debug priority - may be discarded under memory pressure
    Debug = 0,
    /// Info priority - normal events
    #[default]
    Info = 1,
    /// Critical priority - must be delivered
    Critical = 2,
}

impl EventPriority {
    /// Convert to u8 for TLV encoding.
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    /// Create from u8 value.
    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Debug),
            1 => Some(Self::Info),
            2 => Some(Self::Critical),
            _ => None,
        }
    }
}

impl ToTLV for EventPriority {
    fn to_tlv<W: TLVWrite>(&self, tag: &TLVTag, tw: W) -> Result<(), Error> {
        self.as_u8().to_tlv(tag, tw)
    }

    fn tlv_iter(&self, tag: TLVTag) -> impl Iterator<Item = Result<crate::tlv::TLV<'_>, Error>> {
        crate::tlv::TLV::u8(tag, self.as_u8()).into_tlv_iter()
    }
}

impl<'a> FromTLV<'a> for EventPriority {
    fn from_tlv(element: &TLVElement<'a>) -> Result<Self, Error> {
        let value = u8::from_tlv(element)?;
        Self::from_u8(value).ok_or_else(|| crate::error::ErrorCode::Invalid.into())
    }
}

/// Tags for EventDataIB TLV structure per Matter spec.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[repr(u8)]
pub enum EventDataTag {
    /// Event path (endpoint, cluster, event ID)
    Path = 0,
    /// Monotonically increasing event number
    EventNumber = 1,
    /// Event priority (Debug, Info, Critical)
    Priority = 2,
    /// Microseconds since Unix epoch
    EpochTimestamp = 3,
    /// Milliseconds since device boot
    SystemTimestamp = 4,
    /// Delta from previous epoch timestamp
    /// TODO: Implement delta timestamp support for bandwidth optimization
    DeltaEpochTimestamp = 5,
    /// Delta from previous system timestamp
    /// TODO: Implement delta timestamp support for bandwidth optimization
    DeltaSystemTimestamp = 6,
    /// Cluster-specific event payload
    Data = 7,
}

/// Tags for EventReportIB TLV structure per Matter spec.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[repr(u8)]
pub enum EventReportTag {
    /// Error status (when event read fails)
    EventStatus = 0,
    /// Actual event data
    EventData = 1,
}

/// Tags for EventStatusIB TLV structure per Matter spec.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[repr(u8)]
pub enum EventStatusTag {
    /// Event path that failed
    Path = 0,
    /// Error status
    Status = 1,
}

/// Event status for error reporting in event responses.
///
/// Corresponds to `EventStatusIB` in the Matter Interaction Model.
/// Used when an event read fails (e.g., invalid path, access denied).
#[derive(Debug, Clone, PartialEq, Eq, FromTLV, ToTLV)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct EventStatusIB {
    /// The path to the event that failed.
    pub path: EventPath,
    /// The error status.
    pub status: Status,
}

/// Event data with path, number, priority, timestamp, and payload.
///
/// Corresponds to `EventDataIB` in the Matter Interaction Model.
///
/// # Timestamps
///
/// Exactly one timestamp field must be present:
/// - `epoch_timestamp`: Microseconds since Unix epoch (preferred if time is known)
/// - `system_timestamp`: Milliseconds since device boot (fallback)
/// - Delta timestamps: TODO - not yet implemented
///
/// # Example
///
/// ```ignore
/// let event = EventDataIB {
///     path: EventPath {
///         endpoint: Some(1),
///         cluster: Some(0x003B), // GenericSwitch
///         event: Some(0x01),     // InitialPress
///         ..Default::default()
///     },
///     event_number: 42,
///     priority: EventPriority::Info,
///     epoch_timestamp: None,
///     system_timestamp: Some(123456),
///     delta_epoch_timestamp: None,
///     delta_system_timestamp: None,
///     data: Some(&[0x15, 0x24, 0x00, 0x01, 0x18]), // {NewPosition: 1}
/// };
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct EventDataIB<'a> {
    /// The path identifying this event (endpoint, cluster, event ID).
    pub path: EventPath,
    /// Monotonically increasing event number (never resets, even across restarts).
    /// TODO: Implement event number persistence across restarts
    pub event_number: u64,
    /// Event priority level.
    pub priority: EventPriority,
    /// Microseconds since Unix epoch (Jan 1, 1970).
    /// TODO: Implement epoch timestamp support when device has reliable time
    pub epoch_timestamp: Option<u64>,
    /// Milliseconds since device boot.
    pub system_timestamp: Option<u64>,
    /// Delta from previous event's epoch timestamp.
    /// TODO: Implement delta timestamps for bandwidth optimization
    pub delta_epoch_timestamp: Option<u64>,
    /// Delta from previous event's system timestamp.
    /// TODO: Implement delta timestamps for bandwidth optimization
    pub delta_system_timestamp: Option<u64>,
    /// Pre-encoded TLV payload with cluster-specific event data.
    pub data: Option<&'a [u8]>,
}

impl<'a> Default for EventDataIB<'a> {
    fn default() -> Self {
        Self {
            path: EventPath::default(),
            event_number: 0,
            priority: EventPriority::Info,
            epoch_timestamp: None,
            system_timestamp: None,
            delta_epoch_timestamp: None,
            delta_system_timestamp: None,
            data: None,
        }
    }
}

impl<'a> ToTLV for EventDataIB<'a> {
    fn to_tlv<W: TLVWrite>(&self, tag: &TLVTag, mut tw: W) -> Result<(), Error> {
        tw.start_struct(tag)?;

        // Path (tag 0) - required
        self.path
            .to_tlv(&TLVTag::Context(EventDataTag::Path as u8), &mut tw)?;

        // Event number (tag 1) - required
        tw.u64(
            &TLVTag::Context(EventDataTag::EventNumber as u8),
            self.event_number,
        )?;

        // Priority (tag 2) - required
        tw.u8(
            &TLVTag::Context(EventDataTag::Priority as u8),
            self.priority.as_u8(),
        )?;

        // Timestamp - exactly one required (prefer epoch, fallback to system)
        if let Some(ts) = self.epoch_timestamp {
            tw.u64(&TLVTag::Context(EventDataTag::EpochTimestamp as u8), ts)?;
        } else if let Some(ts) = self.system_timestamp {
            tw.u64(&TLVTag::Context(EventDataTag::SystemTimestamp as u8), ts)?;
        } else if let Some(ts) = self.delta_epoch_timestamp {
            // TODO: Delta timestamps not fully implemented
            tw.u64(
                &TLVTag::Context(EventDataTag::DeltaEpochTimestamp as u8),
                ts,
            )?;
        } else if let Some(ts) = self.delta_system_timestamp {
            // TODO: Delta timestamps not fully implemented
            tw.u64(
                &TLVTag::Context(EventDataTag::DeltaSystemTimestamp as u8),
                ts,
            )?;
        }
        // Note: If no timestamp is set, this is technically invalid per spec
        // but we allow it for flexibility during development

        // Data payload (tag 7) - optional, pre-encoded TLV
        // TODO: Implement proper raw TLV data writing
        // The TLVWrite trait doesn't support writing pre-encoded TLV data directly.
        // For now, we skip the data payload. To properly implement this, either:
        // 1. Add a raw() method to TLVWrite trait
        // 2. Use WriteBuf directly which has raw_value()
        // 3. Re-encode the payload from structured data instead of pre-encoded bytes
        if let Some(_data) = self.data {
            // tw.raw(&TLVTag::Context(EventDataTag::Data as u8), data)?;
        }

        tw.end_container()
    }

    fn tlv_iter(&self, _tag: TLVTag) -> impl Iterator<Item = Result<crate::tlv::TLV<'_>, Error>> {
        // TODO: Implement proper tlv_iter for EventDataIB
        // This is complex due to the struct nature; for now, return empty iterator
        // and rely on to_tlv() for serialization
        core::iter::empty()
    }
}

// TODO: Implement FromTLV for EventDataIB to parse events from other Matter nodes
// impl<'a> FromTLV<'a> for EventDataIB<'a> { ... }

/// Event report containing either event data or error status.
///
/// Corresponds to `EventReportIB` in the Matter Interaction Model.
///
/// Each report contains exactly one of:
/// - `event_data`: The actual event (normal case)
/// - `event_status`: An error status (when event read fails)
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct EventReportIB<'a> {
    /// Error status when event read fails.
    pub event_status: Option<EventStatusIB>,
    /// Actual event data.
    pub event_data: Option<EventDataIB<'a>>,
}

impl<'a> Default for EventReportIB<'a> {
    fn default() -> Self {
        Self {
            event_status: None,
            event_data: None,
        }
    }
}

impl<'a> EventReportIB<'a> {
    /// Create an event report with event data.
    pub const fn with_data(event_data: EventDataIB<'a>) -> Self {
        Self {
            event_status: None,
            event_data: Some(event_data),
        }
    }

    /// Create an event report with error status.
    pub const fn with_status(event_status: EventStatusIB) -> Self {
        Self {
            event_status: Some(event_status),
            event_data: None,
        }
    }
}

impl<'a> ToTLV for EventReportIB<'a> {
    fn to_tlv<W: TLVWrite>(&self, tag: &TLVTag, mut tw: W) -> Result<(), Error> {
        tw.start_struct(tag)?;

        if let Some(status) = &self.event_status {
            status.to_tlv(&TLVTag::Context(EventReportTag::EventStatus as u8), &mut tw)?;
        }

        if let Some(data) = &self.event_data {
            data.to_tlv(&TLVTag::Context(EventReportTag::EventData as u8), &mut tw)?;
        }

        tw.end_container()
    }

    fn tlv_iter(&self, _tag: TLVTag) -> impl Iterator<Item = Result<crate::tlv::TLV<'_>, Error>> {
        // TODO: Implement proper tlv_iter for EventReportIB
        // This is complex due to the struct nature; for now, return empty iterator
        // and rely on to_tlv() for serialization
        core::iter::empty()
    }
}

// TODO: Implement FromTLV for EventReportIB to parse event reports from other nodes
// impl<'a> FromTLV<'a> for EventReportIB<'a> { ... }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_priority_conversion() {
        assert_eq!(EventPriority::Debug.as_u8(), 0);
        assert_eq!(EventPriority::Info.as_u8(), 1);
        assert_eq!(EventPriority::Critical.as_u8(), 2);

        assert_eq!(EventPriority::from_u8(0), Some(EventPriority::Debug));
        assert_eq!(EventPriority::from_u8(1), Some(EventPriority::Info));
        assert_eq!(EventPriority::from_u8(2), Some(EventPriority::Critical));
        assert_eq!(EventPriority::from_u8(3), None);
    }

    #[test]
    fn test_event_data_ib_default() {
        let event = EventDataIB::default();
        assert_eq!(event.event_number, 0);
        assert_eq!(event.priority, EventPriority::Info);
        assert!(event.epoch_timestamp.is_none());
        assert!(event.system_timestamp.is_none());
        assert!(event.data.is_none());
    }

    #[test]
    fn test_event_report_ib_constructors() {
        let data = EventDataIB::default();
        let report = EventReportIB::with_data(data);
        assert!(report.event_data.is_some());
        assert!(report.event_status.is_none());

        let status = EventStatusIB {
            path: EventPath::default(),
            status: Status::new(super::super::IMStatusCode::Failure, None),
        };
        let report = EventReportIB::with_status(status);
        assert!(report.event_status.is_some());
        assert!(report.event_data.is_none());
    }
}
