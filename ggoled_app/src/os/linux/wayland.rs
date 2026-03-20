use crate::os::IDLE_TIMEOUT_MS;
use std::{collections::HashMap, io::ErrorKind};
use wayland_client::{
    Connection, Dispatch, EventQueue, Proxy, QueueHandle,
    backend::WaylandError,
    delegate_noop,
    globals::{GlobalListContents, registry_queue_init},
    protocol::{wl_registry, wl_seat},
};
use wayland_protocols::ext::idle_notify::v1::client::{
    ext_idle_notification_v1::{self, ExtIdleNotificationV1},
    ext_idle_notifier_v1::ExtIdleNotifierV1,
};

// (notification handle, is_idle)
type SeatEntry = (ExtIdleNotificationV1, bool);

struct IdleState {
    notifier: ExtIdleNotifierV1,
    seats: HashMap<u32, SeatEntry>,
}

impl IdleState {
    fn add_seat(&mut self, registry: &wl_registry::WlRegistry, qh: &QueueHandle<Self>, name: u32) {
        if self.seats.contains_key(&name) {
            return;
        }
        let seat = registry.bind::<wl_seat::WlSeat, _, _>(name, 1, qh, ());
        let notification = if self.notifier.version() >= 2 {
            self.notifier
                .get_input_idle_notification(IDLE_TIMEOUT_MS, &seat, qh, name)
        } else {
            self.notifier.get_idle_notification(IDLE_TIMEOUT_MS, &seat, qh, name)
        };
        self.seats.insert(name, (notification, false));
    }

    fn remove_seat(&mut self, name: u32) {
        if let Some((notification, _)) = self.seats.remove(&name) {
            notification.destroy();
        }
    }

    fn is_idle(&self) -> bool {
        !self.seats.is_empty() && self.seats.values().all(|(_, idle)| *idle)
    }
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for IdleState {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global { name, interface, .. } if interface == wl_seat::WlSeat::interface().name => {
                state.add_seat(registry, qh, name)
            }
            wl_registry::Event::GlobalRemove { name } => state.remove_seat(name),
            _ => {}
        }
    }
}

delegate_noop!(IdleState: ignore wl_seat::WlSeat);
delegate_noop!(IdleState: ignore ExtIdleNotifierV1);

impl Dispatch<ExtIdleNotificationV1, u32> for IdleState {
    fn event(
        state: &mut Self,
        _: &ExtIdleNotificationV1,
        event: ext_idle_notification_v1::Event,
        name: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let Some((_, idle)) = state.seats.get_mut(name) {
            match event {
                ext_idle_notification_v1::Event::Idled => *idle = true,
                ext_idle_notification_v1::Event::Resumed => *idle = false,
                _ => {}
            }
        }
    }
}

pub struct IdleTracker {
    state: IdleState,
    queue: EventQueue<IdleState>,
}

fn wayland_err(e: impl ToString) -> String {
    e.to_string()
}

impl IdleTracker {
    pub fn new() -> Option<Self> {
        let conn = Connection::connect_to_env().ok()?;
        let (globals, mut queue) = registry_queue_init::<IdleState>(&conn).ok()?;
        let qh = queue.handle();
        let notifier: ExtIdleNotifierV1 = globals.bind(&qh, 1..=2, ()).ok()?;
        let registry = globals.registry().clone();
        let mut state = IdleState {
            notifier,
            seats: HashMap::new(),
        };
        globals.contents().with_list(|globals| {
            for global in globals {
                if global.interface == wl_seat::WlSeat::interface().name {
                    state.add_seat(&registry, &qh, global.name);
                }
            }
        });
        queue.roundtrip(&mut state).ok()?;
        Some(Self { state, queue })
    }

    pub fn get_idle(&mut self) -> Result<bool, String> {
        self.queue.dispatch_pending(&mut self.state).map_err(wayland_err)?;
        if let Some(guard) = self.queue.prepare_read() {
            match guard.read() {
                Ok(_) => {
                    self.queue.dispatch_pending(&mut self.state).map_err(wayland_err)?;
                }
                Err(WaylandError::Io(err)) if err.kind() == ErrorKind::WouldBlock => {}
                Err(err) => return Err(wayland_err(err)),
            }
        }
        Ok(self.state.is_idle())
    }
}
