//! Wayland side: a layer-shell popup drawn into shared memory, keyboard and
//! pointer input, HiDPI scaling and the daemon's event loop.

use crate::paste::{Clipboard, Keyboard, PasteKey};
use crate::ui::{self, Action, Ui};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_keyboard, delegate_layer, delegate_output, delegate_pointer,
    delegate_registry, delegate_seat, delegate_shm,
    output::{OutputHandler, OutputState},
    reexports::{
        calloop::{
            generic::Generic,
            timer::{TimeoutAction, Timer},
            EventLoop, Interest, LoopHandle, Mode, PostAction,
        },
        calloop_wayland_source::WaylandSource,
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers},
        pointer::{PointerEvent, PointerEventKind, PointerHandler, BTN_LEFT},
        Capability, SeatHandler, SeatState,
    },
    shell::{
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
        WaylandSurface,
    },
    shm::{slot::SlotPool, Shm, ShmHandler},
};
use std::io::Read;
use std::os::unix::net::UnixListener;
use std::time::Duration;
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
    Connection, Dispatch, Proxy, QueueHandle,
};
use wayland_protocols::wp::{
    cursor_shape::v1::client::{
        wp_cursor_shape_device_v1::{self, WpCursorShapeDeviceV1},
        wp_cursor_shape_manager_v1::WpCursorShapeManagerV1,
    },
    fractional_scale::v1::client::{
        wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1,
        wp_fractional_scale_v1::{self, WpFractionalScaleV1},
    },
    viewporter::client::{wp_viewport::WpViewport, wp_viewporter::WpViewporter},
};

/// The visible window. Dropping it destroys every surface object.
struct Window {
    layer: LayerSurface,
    pool: Option<SlotPool>,
    fractional: Option<WpFractionalScaleV1>,
    viewport: Option<WpViewport>,
    scale120: u32, // fractional scale × 120
    int_scale: i32,
    configured: bool,
}

impl Drop for Window {
    fn drop(&mut self) {
        if let Some(f) = self.fractional.take() {
            f.destroy();
        }
        if let Some(v) = self.viewport.take() {
            v.destroy();
        }
    }
}

pub struct App {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    compositor: CompositorState,
    shm: Shm,
    layer_shell: LayerShell,
    fractional_mgr: Option<WpFractionalScaleManagerV1>,
    viewporter: Option<WpViewporter>,
    cursor_mgr: Option<WpCursorShapeManagerV1>,
    pub clipboard: Clipboard,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer: Option<wl_pointer::WlPointer>,
    mods: Modifiers,
    win: Option<Window>,
    ui: Ui,
    vkbd: Option<Keyboard>,
    paste_key: PasteKey,
    handle: LoopHandle<'static, App>,
    conn: Connection,
    qh: QueueHandle<App>,
    abgr: bool,
    exit: bool,
}

pub fn run(listener: UnixListener, show: bool, paste_key: PasteKey) -> Result<(), Box<dyn std::error::Error>> {
    let conn = Connection::connect_to_env()?;
    let (globals, queue) = registry_queue_init::<App>(&conn)?;
    let qh = queue.handle();
    let mut event_loop: EventLoop<App> = EventLoop::try_new()?;
    let handle = event_loop.handle();
    WaylandSource::new(conn.clone(), queue).insert(handle.clone())?;

    let vkbd = match Keyboard::new() {
        Ok(k) => Some(k),
        Err(e) => {
            eprintln!("emoji-picker: no /dev/uinput access ({e}); picks will only be copied");
            None
        }
    };

    let mut app = App {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        compositor: CompositorState::bind(&globals, &qh)?,
        shm: Shm::bind(&globals, &qh)?,
        layer_shell: LayerShell::bind(&globals, &qh)?,
        fractional_mgr: globals.bind(&qh, 1..=1, ()).ok(),
        viewporter: globals.bind(&qh, 1..=1, ()).ok(),
        cursor_mgr: globals.bind(&qh, 1..=1, ()).ok(),
        clipboard: Clipboard::new(globals.bind(&qh, 1..=1, ()).ok()),
        keyboard: None,
        pointer: None,
        mods: Modifiers::default(),
        win: None,
        ui: Ui::new(),
        vkbd,
        paste_key,
        handle: handle.clone(),
        conn,
        qh,
        abgr: false,
        exit: false,
    };
    if app.clipboard.manager.is_none() {
        eprintln!("emoji-picker: compositor lacks ext-data-control; pasting won't work");
    }

    // Commands from `emoji-picker --toggle` etc.
    listener.set_nonblocking(true)?;
    handle.insert_source(Generic::new(listener, Interest::READ, Mode::Level), |_, l, app: &mut App| {
        while let Ok((mut s, _)) = l.accept() {
            let _ = s.set_read_timeout(Some(Duration::from_millis(200)));
            let mut b = [0u8; 1];
            if s.read_exact(&mut b).is_ok() {
                match b[0] {
                    b't' => app.toggle(),
                    b's' => app.show(),
                    b'q' => app.exit = true,
                    _ => {}
                }
            }
        }
        Ok(PostAction::Continue)
    })?;

    event_loop.dispatch(Duration::from_millis(50), &mut app)?; // learn seats and formats
    app.abgr = app.shm.formats().contains(&wl_shm::Format::Abgr8888);
    app.ui.warm_up();
    if show {
        app.show();
    }
    trim_memory();
    while !app.exit {
        event_loop.dispatch(None, &mut app)?;
    }
    Ok(())
}

fn trim_memory() {
    unsafe {
        libc::malloc_trim(0);
    }
}

impl App {
    fn toggle(&mut self) {
        if self.win.is_some() { self.hide() } else { self.show() }
    }

    fn show(&mut self) {
        if self.win.is_some() {
            return;
        }
        let qh = &self.qh;
        let surface = self.compositor.create_surface(qh);
        let fractional = self.fractional_mgr.as_ref().map(|m| m.get_fractional_scale(&surface, qh, ()));
        let viewport = self.viewporter.as_ref().map(|v| v.get_viewport(&surface, qh, ()));
        let layer = self.layer_shell.create_layer_surface(qh, surface, Layer::Overlay, Some("emoji-picker"), None);
        layer.set_anchor(Anchor::empty()); // centered
        layer.set_size(ui::WIDTH, ui::HEIGHT);
        layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
        layer.commit();
        self.ui.reset();
        self.win = Some(Window {
            layer,
            pool: None,
            fractional,
            viewport,
            scale120: 120,
            int_scale: 1,
            configured: false,
        });
    }

    fn hide(&mut self) {
        if self.win.take().is_some() {
            self.ui.release();
            let _ = self.conn.flush();
            trim_memory();
        }
    }

    fn act(&mut self, a: Action) {
        match a {
            Action::None => {}
            Action::Redraw => self.draw(),
            Action::Close => self.hide(),
            Action::Pick(item) => self.pick(item),
        }
    }

    fn pick(&mut self, item: u32) {
        self.ui.add_recent(item);
        let copied = self.clipboard.set(crate::data::text(item), &self.qh);
        self.hide();
        if !copied || self.vkbd.is_none() {
            return;
        }
        // Give the compositor a moment to hand focus back to the previous window.
        let _ = self.conn.flush();
        let _ = self.handle.insert_source(Timer::from_duration(Duration::from_millis(70)), |_, _, app| {
            if let Some(k) = &app.vkbd {
                k.paste(app.paste_key);
            }
            TimeoutAction::Drop
        });
    }

    fn on_key(&mut self, ev: KeyEvent) {
        if self.win.is_none() {
            return;
        }
        let a = self.ui.key(ev.keysym, ev.utf8.as_deref(), &self.mods);
        self.act(a);
    }

    fn draw(&mut self) {
        let abgr = self.abgr;
        let Some(win) = &mut self.win else { return };
        if !win.configured {
            return;
        }
        let (scale, int_scale) = if win.viewport.is_some() && win.fractional.is_some() {
            (win.scale120 as f32 / 120.0, 1)
        } else {
            (win.int_scale as f32, win.int_scale)
        };
        let pw = (ui::WIDTH as f32 * scale).round() as u32;
        let ph = (ui::HEIGHT as f32 * scale).round() as u32;
        let stride = pw as i32 * 4;
        let pool = match &mut win.pool {
            Some(p) => p,
            None => win.pool.insert(SlotPool::new((pw * ph * 4) as usize, &self.shm).expect("shm pool")),
        };
        let format = if abgr { wl_shm::Format::Abgr8888 } else { wl_shm::Format::Argb8888 };
        let Ok((buffer, canvas)) = pool.create_buffer(pw as i32, ph as i32, stride, format) else { return };
        {
            let Some(mut pm) = resvg::tiny_skia::PixmapMut::from_bytes(canvas, pw, ph) else { return };
            self.ui.draw(&mut pm, scale);
        }
        if !abgr {
            // tiny-skia writes RGBA; ARGB8888 in memory is BGRA.
            for px in canvas.chunks_exact_mut(4) {
                px.swap(0, 2);
            }
        }
        let surface = win.layer.wl_surface();
        if let Some(vp) = &win.viewport {
            vp.set_destination(ui::WIDTH as i32, ui::HEIGHT as i32);
        }
        surface.set_buffer_scale(int_scale);
        surface.damage_buffer(0, 0, pw as i32, ph as i32);
        if buffer.attach_to(surface).is_ok() {
            win.layer.commit();
        }
    }
}

// ---------------------------------------------------------------- handlers

impl CompositorHandler for App {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, factor: i32) {
        if let Some(w) = &mut self.win {
            if w.int_scale != factor {
                w.int_scale = factor;
                w.pool = None;
                self.draw();
            }
        }
    }
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for App {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.hide();
    }
    fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface, _: LayerSurfaceConfigure, _: u32) {
        if let Some(w) = &mut self.win {
            w.configured = true;
        }
        self.draw();
    }
}

impl SeatHandler for App {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(&mut self, _: &Connection, qh: &QueueHandle<Self>, seat: wl_seat::WlSeat, cap: Capability) {
        if self.clipboard.device.is_none() {
            if let Some(m) = &self.clipboard.manager {
                self.clipboard.device = Some(m.get_data_device(&seat, qh, ()));
            }
        }
        if cap == Capability::Keyboard && self.keyboard.is_none() {
            let cb = Box::new(|app: &mut App, _: &wl_keyboard::WlKeyboard, ev: KeyEvent| app.on_key(ev));
            self.keyboard = self.seat_state.get_keyboard_with_repeat(qh, &seat, None, self.handle.clone(), cb).ok();
        }
        if cap == Capability::Pointer && self.pointer.is_none() {
            self.pointer = self.seat_state.get_pointer(qh, &seat).ok();
        }
    }
    fn remove_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat, cap: Capability) {
        if cap == Capability::Keyboard {
            if let Some(k) = self.keyboard.take() {
                k.release();
            }
        }
        if cap == Capability::Pointer {
            if let Some(p) = self.pointer.take() {
                p.release();
            }
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl KeyboardHandler for App {
    fn enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: &wl_surface::WlSurface, _: u32, _: &[u32], _: &[Keysym]) {}
    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: &wl_surface::WlSurface, _: u32) {}
    fn press_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, ev: KeyEvent) {
        self.on_key(ev);
    }
    fn release_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, _: KeyEvent) {}
    fn update_modifiers(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, m: Modifiers, _: u32) {
        self.mods = m;
    }
}

impl PointerHandler for App {
    fn pointer_frame(&mut self, _: &Connection, qh: &QueueHandle<Self>, pointer: &wl_pointer::WlPointer, events: &[PointerEvent]) {
        let Some(win) = &self.win else { return };
        let surface = win.layer.wl_surface().clone();
        let mut redraw = false;
        for ev in events {
            if ev.surface != surface {
                continue;
            }
            let (x, y) = (ev.position.0 as f32, ev.position.1 as f32);
            let a = match ev.kind {
                PointerEventKind::Enter { serial } => {
                    if let Some(m) = &self.cursor_mgr {
                        let d = m.get_pointer(pointer, qh, ());
                        d.set_shape(serial, wp_cursor_shape_device_v1::Shape::Default);
                        d.destroy();
                    }
                    self.ui.motion(x, y)
                }
                PointerEventKind::Leave { .. } => self.ui.leave(),
                PointerEventKind::Motion { .. } => self.ui.motion(x, y),
                PointerEventKind::Press { button: BTN_LEFT, .. } => self.ui.click(x, y),
                PointerEventKind::Axis { vertical, .. } => {
                    let dy = if vertical.discrete != 0 { vertical.discrete as f64 * 44.0 } else { vertical.absolute * 1.5 };
                    self.ui.scroll_by(dy as f32)
                }
                _ => Action::None,
            };
            match a {
                Action::Redraw => redraw = true,
                Action::None => {}
                other => {
                    self.act(other);
                    return;
                }
            }
        }
        if redraw {
            self.draw();
        }
    }
}

impl ShmHandler for App {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

delegate_compositor!(App);
delegate_output!(App);
delegate_shm!(App);
delegate_seat!(App);
delegate_keyboard!(App);
delegate_pointer!(App);
delegate_layer!(App);
delegate_registry!(App);

// Protocol objects without interesting events.
macro_rules! ignore_events {
    ($($t:ty),*) => {$(
        impl Dispatch<$t, ()> for App {
            fn event(_: &mut Self, _: &$t, _: <$t as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
        }
    )*};
}
ignore_events!(WpFractionalScaleManagerV1, WpViewporter, WpViewport, WpCursorShapeManagerV1, WpCursorShapeDeviceV1);

impl Dispatch<WpFractionalScaleV1, ()> for App {
    fn event(app: &mut Self, _: &WpFractionalScaleV1, ev: wp_fractional_scale_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let wp_fractional_scale_v1::Event::PreferredScale { scale } = ev {
            if let Some(w) = &mut app.win {
                if w.scale120 != scale {
                    w.scale120 = scale;
                    w.pool = None;
                    app.draw();
                }
            }
        }
    }
}
