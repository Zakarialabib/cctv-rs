//! HAL Hot-Plug event source.
//!
//! Emits domain events ([`DomainEvent::DeviceAttached`], [`DomainEvent::DeviceDropped`])
//! when hardware nodes are connected or unplugged.

use cctv_core::DomainEvent;
use std::collections::VecDeque;

/// Hot-plug HAL event queue source.
#[derive(Debug, Default)]
pub struct HalEventSource {
    events: VecDeque<DomainEvent>,
}

impl HalEventSource {
    /// Create a new HAL event source.
    pub fn new() -> Self {
        Self::default()
    }

    /// Simulate a hardware device attach event.
    pub fn attach(&mut self, camera_id: impl Into<String>, driver: impl Into<String>) {
        self.events.push_back(DomainEvent::DeviceAttached {
            camera_id: camera_id.into(),
            driver: driver.into(),
        });
    }

    /// Simulate a hardware device drop / hot-unplug event.
    pub fn drop(&mut self, camera_id: impl Into<String>) {
        self.events.push_back(DomainEvent::DeviceDropped {
            camera_id: camera_id.into(),
        });
    }

    /// Pop the next domain event, if any.
    pub fn poll_event(&mut self) -> Option<DomainEvent> {
        self.events.pop_front()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hal_event_source_emits_attached_and_dropped_events() {
        let mut hal = HalEventSource::new();
        hal.attach("cam1", "uvc");
        hal.drop("cam1");

        assert_eq!(
            hal.poll_event(),
            Some(DomainEvent::DeviceAttached {
                camera_id: "cam1".into(),
                driver: "uvc".into()
            })
        );
        assert_eq!(
            hal.poll_event(),
            Some(DomainEvent::DeviceDropped {
                camera_id: "cam1".into()
            })
        );
        assert_eq!(hal.poll_event(), None);
    }
}
