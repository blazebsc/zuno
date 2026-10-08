//! Custom Masonry widgets + Xilem views the benchmark needs that xilem 0.4
//! doesn't ship: a keyboard-focus sink (global shortcuts + resize reporting),
//! a hover-aware button with overlay painting (hover backgrounds, card play
//! scrim, animated playing bars), window chrome (drag strip, min/max/close),
//! vector glyphs, and a scroll-position-drivable portal for the bench.

use std::any::TypeId;
use std::time::Instant;

use masonry::accesskit::{self, Node, Role};
use masonry::core::keyboard::{Key, KeyState, NamedKey};
use masonry::core::{
    AccessCtx, AccessEvent, BoxConstraints, ChildrenIds, EventCtx, HasProperty,
    LayoutCtx, NewWidget, PaintCtx, PointerButton, PointerButtonEvent, PointerEvent, PropertiesMut,
    PropertiesRef, Property, RegisterCtx, TextEvent, Update, UpdateCtx, Widget, WidgetId,
    WidgetMut, WidgetPod,
};
use masonry::kurbo::{Affine, BezPath, Circle, Point, Rect, Size, Stroke};
use masonry::peniko::color::{AlphaColor, Srgb};
use masonry::peniko::{Color, Fill};
use masonry::properties::{
    ActiveBackground, Background, BorderColor, BorderWidth, BoxShadow, CornerRadius,
    DisabledBackground, Padding,
};
use masonry::vello::Scene;
use tracing::{Span, trace_span};

// Xilem view plumbing
use xilem::core::{MessageContext, MessageResult, Mut, View, ViewId, ViewMarker, ViewPathTracker};
use xilem::{Pod, ViewCtx, WidgetView};

pub const CHILD_VIEW_ID: ViewId = ViewId::new(0);

// ---------------------------------------------------------------------------
// A custom property: background while hovered (masonry only ships hovered
// *border* color; Zuno's hover language is a surface change).
#[derive(Clone, Debug, PartialEq)]
pub struct HoveredBackground(pub Background);

impl Default for HoveredBackground {
    fn default() -> Self {
        Self::static_default().clone()
    }
}

impl Property for HoveredBackground {
    fn static_default() -> &'static Self {
        static DEFAULT: HoveredBackground =
            HoveredBackground(Background::Color(AlphaColor::<Srgb>::TRANSPARENT));
        &DEFAULT
    }
}

impl HoveredBackground {
    /// Helper to be called in `Widget::property_changed`.
    pub fn prop_changed(ctx: &mut UpdateCtx<'_>, property_type: TypeId) {
        if property_type != TypeId::of::<Self>() {
            return;
        }
        ctx.request_paint_only();
    }
}

// ---------------------------------------------------------------------------
// --- KeySink: root container -----------------------------------------------
// Receives all keyboard events while no text input is focused (it is the
// window's focus fallback), translates them into `KeyAction`s for the app,
// and reports its (i.e. the window content's) size changes for responsive
// layout.

#[derive(Clone, Debug, PartialEq)]
pub enum KeyAction {
    TogglePlay,
    SeekDelta(f64),
    Escape,
    Mute,
    ToggleQueue,
    Shuffle,
    Repeat,
    FocusSearch,
    Resized(f64, f64),
}

pub struct KeySink {
    child: WidgetPod<dyn Widget>,
    last_size: Option<Size>,
}

impl KeySink {
    pub fn new(child: NewWidget<dyn Widget>) -> Self {
        Self { child: child.to_pod(), last_size: None }
    }

    pub fn child_mut<'t>(this: &'t mut WidgetMut<'_, Self>) -> WidgetMut<'t, dyn Widget> {
        this.ctx.get_mut(&mut this.widget.child)
    }
}

impl Widget for KeySink {
    type Action = KeyAction;

    fn on_text_event(&mut self, ctx: &mut EventCtx<'_>, _props: &mut PropertiesMut<'_>, event: &TextEvent) {
        let TextEvent::Keyboard(k) = event else { return };
        if k.state != KeyState::Down {
            return;
        }
        let ctrl = k.modifiers.ctrl();
        let is_arrow = matches!(&k.key, Key::Named(NamedKey::ArrowLeft | NamedKey::ArrowRight));
        if k.repeat && !is_arrow {
            return; // no toggle-spam while holding letter keys
        }
        let action = match &k.key {
            Key::Character(c) if ctrl && c.eq_ignore_ascii_case("k") => Some(KeyAction::FocusSearch),
            Key::Character(c) if c.as_str() == " " => Some(KeyAction::TogglePlay),
            Key::Character(c) if c.eq_ignore_ascii_case("m") => Some(KeyAction::Mute),
            Key::Character(c) if c.eq_ignore_ascii_case("q") => Some(KeyAction::ToggleQueue),
            Key::Character(c) if c.eq_ignore_ascii_case("s") => Some(KeyAction::Shuffle),
            Key::Character(c) if c.eq_ignore_ascii_case("r") => Some(KeyAction::Repeat),
            Key::Named(NamedKey::ArrowLeft) => Some(KeyAction::SeekDelta(-10.0)),
            Key::Named(NamedKey::ArrowRight) => Some(KeyAction::SeekDelta(10.0)),
            Key::Named(NamedKey::Escape) => Some(KeyAction::Escape),
            _ => None,
        };
        if let Some(a) = action {
            ctx.submit_action::<Self::Action>(a);
            ctx.set_handled();
        }
    }

    fn register_children(&mut self, ctx: &mut RegisterCtx<'_>) {
        ctx.register_child(&mut self.child);
    }

    fn layout(&mut self, ctx: &mut LayoutCtx<'_>, _props: &mut PropertiesMut<'_>, bc: &BoxConstraints) -> Size {
        let size = ctx.run_layout(&mut self.child, bc);
        ctx.place_child(&mut self.child, Point::ORIGIN);
        if self.last_size != Some(size) {
            self.last_size = Some(size);
            ctx.submit_action::<Self::Action>(KeyAction::Resized(size.width, size.height));
        }
        size
    }

    fn paint(&mut self, _ctx: &mut PaintCtx<'_>, _props: &PropertiesRef<'_>, _scene: &mut Scene) {}

    fn accessibility_role(&self) -> Role {
        Role::GenericContainer
    }

    fn accessibility(&mut self, _ctx: &mut AccessCtx<'_>, _props: &PropertiesRef<'_>, _node: &mut Node) {}

    fn children_ids(&self) -> ChildrenIds {
        ChildrenIds::from_slice(&[self.child.id()])
    }

    fn make_trace_span(&self, id: WidgetId) -> Span {
        trace_span!("KeySink", id = id.trace())
    }
}

/// The view for [`KeySink`]. `handler` receives every translated key action.
pub struct KeySinkView<Child, Handler> {
    child: Child,
    handler: Handler,
}

pub fn key_sink<Child, State, Action, Handler>(
    handler: Handler,
    child: Child,
) -> KeySinkView<Child, Handler>
where
    Child: WidgetView<State, Action>,
    Handler: Fn(&mut State, KeyAction) -> Action + Send + Sync + 'static,
{
    KeySinkView { child, handler }
}

impl<Child, Handler> ViewMarker for KeySinkView<Child, Handler> {}

impl<Child, State, Action, Handler> View<State, Action, ViewCtx> for KeySinkView<Child, Handler>
where
    State: 'static,
    Action: 'static,
    Child: WidgetView<State, Action>,
    Handler: Fn(&mut State, KeyAction) -> Action + Send + Sync + 'static,
{
    type Element = Pod<KeySink>;
    type ViewState = Child::ViewState;

    fn build(&self, ctx: &mut ViewCtx, app_state: &mut State) -> (Self::Element, Self::ViewState) {
        let (child_pod, child_state) =
            ctx.with_id(CHILD_VIEW_ID, |ctx| self.child.build(ctx, app_state));
        let pod = ctx.with_action_widget(|ctx| ctx.create_pod(KeySink::new(child_pod.new_widget.erased())));
        (pod, child_state)
    }

    fn rebuild(&self, prev: &Self, view_state: &mut Self::ViewState, ctx: &mut ViewCtx, mut element: Mut<'_, Self::Element>, app_state: &mut State) {
        ctx.with_id(CHILD_VIEW_ID, |ctx| {
            self.child.rebuild(
                &prev.child,
                view_state,
                ctx,
                KeySink::child_mut(&mut element).downcast(),
                app_state,
            );
        });
    }

    fn teardown(&self, view_state: &mut Self::ViewState, ctx: &mut ViewCtx, mut element: Mut<'_, Self::Element>) {
        ctx.with_id(CHILD_VIEW_ID, |ctx| {
            self.child.teardown(view_state, ctx, KeySink::child_mut(&mut element).downcast());
        });
        ctx.teardown_leaf(element);
    }

    fn message(&self, view_state: &mut Self::ViewState, message: &mut MessageContext, mut element: Mut<'_, Self::Element>, app_state: &mut State) -> MessageResult<Action> {
        match message.take_first() {
            Some(CHILD_VIEW_ID) => {
                self.child.message(
                    view_state,
                    message,
                    KeySink::child_mut(&mut element).downcast(),
                    app_state,
                )
            }
            _ => match message.take_message::<KeyAction>() {
                Some(action) => MessageResult::Action((self.handler)(app_state, *action)),
                None => {
                    tracing::error!("Wrong message type in KeySinkView::message");
                    MessageResult::Stale
                }
            },
        }
    }
}

// ---------------------------------------------------------------------------
// --- HotButton: hover-aware button with overlay painting --------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Overlay {
    None,
    /// Hover-only: dark scrim + round primary play pill over the child.
    CardPlay,
    /// Always painted: 3-bar "now playing" indicator at the row's index column.
    Bars(u8),
}

#[derive(Clone, Debug, PartialEq)]
pub struct HotPress;

pub struct HotButton {
    child: WidgetPod<dyn Widget>,
    pub overlay: Overlay,
}

impl HotButton {
    pub fn new(child: NewWidget<dyn Widget>, overlay: Overlay) -> Self {
        Self { child: child.to_pod(), overlay }
    }

    pub fn child_mut<'t>(this: &'t mut WidgetMut<'_, Self>) -> WidgetMut<'t, dyn Widget> {
        this.ctx.get_mut(&mut this.widget.child)
    }
}

// Style properties HotButton honours.
impl HasProperty<Background> for HotButton {}
impl HasProperty<HoveredBackground> for HotButton {}
impl HasProperty<ActiveBackground> for HotButton {}
impl HasProperty<DisabledBackground> for HotButton {}
impl HasProperty<CornerRadius> for HotButton {}
impl HasProperty<Padding> for HotButton {}
impl HasProperty<BoxShadow> for HotButton {}
impl HasProperty<BorderColor> for HotButton {}
impl HasProperty<BorderWidth> for HotButton {}

impl Widget for HotButton {
    type Action = HotPress;

    fn on_pointer_event(&mut self, ctx: &mut EventCtx<'_>, _props: &mut PropertiesMut<'_>, event: &PointerEvent) {
        match event {
            PointerEvent::Down(..) => {
                ctx.capture_pointer();
                ctx.request_paint_only();
            }
            PointerEvent::Up(PointerButtonEvent { button: Some(PointerButton::Primary), .. }) => {
                if ctx.is_active() && ctx.is_hovered() {
                    ctx.submit_action::<Self::Action>(HotPress);
                    ctx.set_handled();
                }
                ctx.request_paint_only();
            }
            _ => (),
        }
    }

    fn on_text_event(&mut self, ctx: &mut EventCtx<'_>, _props: &mut PropertiesMut<'_>, event: &TextEvent) {
        // Keyboard activation when focused (mirrors masonry's Button).
        if let TextEvent::Keyboard(k) = event {
            if k.state.is_up() && ctx.is_focus_target() {
                let activated = matches!(&k.key, Key::Character(c) if c == " ")
                    || matches!(&k.key, Key::Named(NamedKey::Enter));
                if activated {
                    ctx.submit_action::<Self::Action>(HotPress);
                    ctx.set_handled();
                }
            }
        }
    }

    fn on_access_event(&mut self, ctx: &mut EventCtx<'_>, _props: &mut PropertiesMut<'_>, event: &AccessEvent) {
        if event.action == accesskit::Action::Click {
            ctx.submit_action::<Self::Action>(HotPress);
        }
    }

    fn update(&mut self, ctx: &mut UpdateCtx<'_>, _props: &mut PropertiesMut<'_>, event: &Update) {
        if matches!(
            event,
            Update::HoveredChanged(_) | Update::ActiveChanged(_) | Update::FocusChanged(_) | Update::DisabledChanged(_)
        ) {
            // post_paint (scrim/bars) must re-run too, so request the full render.
            ctx.request_render();
        }
    }

    fn property_changed(&mut self, ctx: &mut UpdateCtx<'_>, property_type: TypeId) {
        Background::prop_changed(ctx, property_type);
        HoveredBackground::prop_changed(ctx, property_type);
        ActiveBackground::prop_changed(ctx, property_type);
        DisabledBackground::prop_changed(ctx, property_type);
        CornerRadius::prop_changed(ctx, property_type);
        Padding::prop_changed(ctx, property_type);
        BoxShadow::prop_changed(ctx, property_type);
        BorderColor::prop_changed(ctx, property_type);
        BorderWidth::prop_changed(ctx, property_type);
    }

    fn register_children(&mut self, ctx: &mut RegisterCtx<'_>) {
        ctx.register_child(&mut self.child);
    }

    fn layout(&mut self, ctx: &mut LayoutCtx<'_>, props: &mut PropertiesMut<'_>, bc: &BoxConstraints) -> Size {
        let border = props.get::<BorderWidth>();
        let padding = props.get::<Padding>();
        let child_bc = padding.layout_down(border.layout_down(bc.loosen()));
        let child_size = ctx.run_layout(&mut self.child, &child_bc);
        let (inner, _baseline) = padding.layout_up(child_size, 0.0);
        let (size, _baseline) = border.layout_up(inner, _baseline);
        // Zuno rows/controls want exact sizes; center the child like masonry's Button.
        let size = bc.constrain(size);
        let offset = (size.to_vec2() - child_size.to_vec2()) / 2.0;
        ctx.place_child(&mut self.child, offset.to_point());

        let shadow = props.get::<BoxShadow>();
        if shadow.is_visible() {
            ctx.set_paint_insets(shadow.get_insets());
        }
        size
    }

    fn paint(&mut self, ctx: &mut PaintCtx<'_>, props: &PropertiesRef<'_>, scene: &mut Scene) {
        let size = ctx.size();
        let corner = props.get::<CornerRadius>().radius;
        let bg: &Background = if ctx.is_disabled() {
            &props.get::<DisabledBackground>().0
        } else if ctx.is_active() {
            &props.get::<ActiveBackground>().0
        } else if ctx.is_hovered() {
            &props.get::<HoveredBackground>().0
        } else {
            props.get::<Background>()
        };
        let rect = size.to_rect().to_rounded_rect(corner);
        let brush = bg.get_peniko_brush_for_rect(rect.rect());
        scene.fill(Fill::NonZero, Affine::IDENTITY, &brush, None, &rect);
    }

    fn post_paint(&mut self, ctx: &mut PaintCtx<'_>, _props: &PropertiesRef<'_>, scene: &mut Scene) {
        let size = ctx.size();
        match self.overlay {
            Overlay::CardPlay if ctx.is_hovered() && !ctx.is_disabled() => {
                // Dark scrim at 50% over the artwork area.
                let scrim = Color::from_rgba8(0x0a, 0x0a, 0x0a, 0x80);
                scene.fill(Fill::NonZero, Affine::IDENTITY, scrim, None, &size.to_rect());
                // 48px round primary play pill.
                let r = 24.0;
                let center = Point::new(size.width / 2.0, size.height / 2.0);
                scene.fill(Fill::NonZero, Affine::IDENTITY, Color::from_rgb8(0xff, 0x00, 0x33), None, &Circle::new(center, r));
                let mut tri = BezPath::new();
                tri.move_to((center.x - 5.0, center.y - 9.0));
                tri.line_to((center.x - 5.0, center.y + 9.0));
                tri.line_to((center.x + 10.0, center.y));
                tri.close_path();
                scene.fill(Fill::NonZero, Affine::IDENTITY, Color::from_rgb8(0xfa, 0xfa, 0xfa), None, &tri);
            }
            Overlay::Bars(phase) => {
                // Animated 3-bar indicator over the index column (left 32px).
                let x = 16.0;
                let cy = size.height / 2.0;
                let heights = [10.0, 16.0, 7.0];
                let phase = phase as usize;
                for i in 0..3 {
                    let h = heights[(i + phase) % 3];
                    let bx = x - 7.0 + i as f64 * 5.5;
                    let bar = Rect::new(bx, cy - h / 2.0, bx + 3.0, cy + h / 2.0).to_rounded_rect(1.5);
                    scene.fill(Fill::NonZero, Affine::IDENTITY, Color::from_rgb8(0xff, 0x00, 0x33), None, &bar);
                }
            }
            _ => {}
        }
    }

    fn accessibility_role(&self) -> Role {
        Role::Button
    }

    fn accessibility(&mut self, _ctx: &mut AccessCtx<'_>, _props: &PropertiesRef<'_>, node: &mut Node) {
        node.add_action(accesskit::Action::Click);
    }

    fn children_ids(&self) -> ChildrenIds {
        ChildrenIds::from_slice(&[self.child.id()])
    }

    fn make_trace_span(&self, id: WidgetId) -> Span {
        trace_span!("HotButton", id = id.trace())
    }
}

/// The view for [`HotButton`] — Zuno's workhorse interactive container.
pub struct Hot<Child, Handler> {
    child: Child,
    handler: Handler,
    overlay: Overlay,
}

pub fn hot<Child, State, Action, Handler>(child: Child, handler: Handler) -> Hot<Child, Handler>
where
    Child: WidgetView<State, Action>,
    Handler: Fn(&mut State) -> Action + Send + Sync + 'static,
{
    Hot { child, handler, overlay: Overlay::None }
}

impl<Child, Handler> Hot<Child, Handler> {
    pub fn overlay(mut self, overlay: Overlay) -> Self {
        self.overlay = overlay;
        self
    }
}

impl<Child, Handler> ViewMarker for Hot<Child, Handler> {}

impl<Child, State, Action, Handler> View<State, Action, ViewCtx> for Hot<Child, Handler>
where
    State: 'static,
    Action: 'static,
    Child: WidgetView<State, Action>,
    Handler: Fn(&mut State) -> Action + Send + Sync + 'static,
{
    type Element = Pod<HotButton>;
    type ViewState = Child::ViewState;

    fn build(&self, ctx: &mut ViewCtx, app_state: &mut State) -> (Self::Element, Self::ViewState) {
        let (child_pod, child_state) =
            ctx.with_id(CHILD_VIEW_ID, |ctx| self.child.build(ctx, app_state));
        let pod = ctx.with_action_widget(|ctx| {
            ctx.create_pod(HotButton::new(child_pod.new_widget.erased(), self.overlay))
        });
        (pod, child_state)
    }

    fn rebuild(&self, prev: &Self, view_state: &mut Self::ViewState, ctx: &mut ViewCtx, mut element: Mut<'_, Self::Element>, app_state: &mut State) {
        if prev.overlay != self.overlay {
            element.widget.overlay = self.overlay;
            element.ctx.request_render();
        }
        ctx.with_id(CHILD_VIEW_ID, |ctx| {
            self.child.rebuild(
                &prev.child,
                view_state,
                ctx,
                HotButton::child_mut(&mut element).downcast(),
                app_state,
            );
        });
    }

    fn teardown(&self, view_state: &mut Self::ViewState, ctx: &mut ViewCtx, mut element: Mut<'_, Self::Element>) {
        ctx.with_id(CHILD_VIEW_ID, |ctx| {
            self.child.teardown(view_state, ctx, HotButton::child_mut(&mut element).downcast());
        });
        ctx.teardown_leaf(element);
    }

    fn message(&self, view_state: &mut Self::ViewState, message: &mut MessageContext, mut element: Mut<'_, Self::Element>, app_state: &mut State) -> MessageResult<Action> {
        match message.take_first() {
            Some(CHILD_VIEW_ID) => {
                self.child.message(
                    view_state,
                    message,
                    HotButton::child_mut(&mut element).downcast(),
                    app_state,
                )
            }
            _ => match message.take_message::<HotPress>() {
                Some(_) => MessageResult::Action((self.handler)(app_state)),
                None => {
                    tracing::error!("Wrong message type in Hot::message");
                    MessageResult::Stale
                }
            },
        }
    }
}

// ---------------------------------------------------------------------------
// --- DragStrip: titlebar drag region + double-click maximize ---------------

pub struct DragStrip {
    child: WidgetPod<dyn Widget>,
    last_click: Option<Instant>,
}

impl DragStrip {
    pub fn new(child: NewWidget<dyn Widget>) -> Self {
        Self { child: child.to_pod(), last_click: None }
    }

    pub fn child_mut<'t>(this: &'t mut WidgetMut<'_, Self>) -> WidgetMut<'t, dyn Widget> {
        this.ctx.get_mut(&mut this.widget.child)
    }
}

impl Widget for DragStrip {
    type Action = HotPress;

    fn on_pointer_event(&mut self, ctx: &mut EventCtx<'_>, _props: &mut PropertiesMut<'_>, event: &PointerEvent) {
        if let PointerEvent::Down(PointerButtonEvent { button: Some(PointerButton::Primary), .. }) = event {
            let now = Instant::now();
            let dbl = self.last_click.map(|t| now.duration_since(t).as_millis() < 400).unwrap_or(false);
            self.last_click = Some(now);
            if dbl {
                ctx.toggle_maximized();
            } else {
                ctx.drag_window();
            }
            ctx.set_handled();
        }
    }

    fn register_children(&mut self, ctx: &mut RegisterCtx<'_>) {
        ctx.register_child(&mut self.child);
    }

    fn layout(&mut self, ctx: &mut LayoutCtx<'_>, _props: &mut PropertiesMut<'_>, bc: &BoxConstraints) -> Size {
        let size = ctx.run_layout(&mut self.child, bc);
        ctx.place_child(&mut self.child, Point::ORIGIN);
        size
    }

    fn paint(&mut self, _ctx: &mut PaintCtx<'_>, _props: &PropertiesRef<'_>, _scene: &mut Scene) {}

    fn accessibility_role(&self) -> Role {
        Role::TitleBar
    }

    fn accessibility(&mut self, _ctx: &mut AccessCtx<'_>, _props: &PropertiesRef<'_>, _node: &mut Node) {}

    fn children_ids(&self) -> ChildrenIds {
        ChildrenIds::from_slice(&[self.child.id()])
    }

    fn make_trace_span(&self, id: WidgetId) -> Span {
        trace_span!("DragStrip", id = id.trace())
    }
}

pub struct DragStripView<Child> {
    child: Child,
}

pub fn drag_strip<Child, State, Action>(child: Child) -> DragStripView<Child>
where
    Child: WidgetView<State, Action>,
{
    DragStripView { child }
}

impl<Child> ViewMarker for DragStripView<Child> {}

impl<Child, State, Action> View<State, Action, ViewCtx> for DragStripView<Child>
where
    State: 'static,
    Action: 'static,
    Child: WidgetView<State, Action>,
{
    type Element = Pod<DragStrip>;
    type ViewState = Child::ViewState;

    fn build(&self, ctx: &mut ViewCtx, app_state: &mut State) -> (Self::Element, Self::ViewState) {
        let (child_pod, child_state) =
            ctx.with_id(CHILD_VIEW_ID, |ctx| self.child.build(ctx, app_state));
        (ctx.create_pod(DragStrip::new(child_pod.new_widget.erased())), child_state)
    }

    fn rebuild(&self, prev: &Self, view_state: &mut Self::ViewState, ctx: &mut ViewCtx, mut element: Mut<'_, Self::Element>, app_state: &mut State) {
        ctx.with_id(CHILD_VIEW_ID, |ctx| {
            self.child.rebuild(
                &prev.child,
                view_state,
                ctx,
                DragStrip::child_mut(&mut element).downcast(),
                app_state,
            );
        });
    }

    fn teardown(&self, view_state: &mut Self::ViewState, ctx: &mut ViewCtx, mut element: Mut<'_, Self::Element>) {
        ctx.with_id(CHILD_VIEW_ID, |ctx| {
            self.child.teardown(view_state, ctx, DragStrip::child_mut(&mut element).downcast());
        });
    }

    fn message(&self, view_state: &mut Self::ViewState, message: &mut MessageContext, mut element: Mut<'_, Self::Element>, app_state: &mut State) -> MessageResult<Action> {
        self.child.message(
            view_state,
            message,
            DragStrip::child_mut(&mut element).downcast(),
            app_state,
        )
    }
}

// ---------------------------------------------------------------------------
// --- WindowControl: min / max / close button --------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowControlKind {
    Minimize,
    Maximize,
    Close,
}

pub struct WindowControl {
    child: WidgetPod<dyn Widget>,
    kind: WindowControlKind,
}

impl WindowControl {
    pub fn new(child: NewWidget<dyn Widget>, kind: WindowControlKind) -> Self {
        Self { child: child.to_pod(), kind }
    }

    pub fn child_mut<'t>(this: &'t mut WidgetMut<'_, Self>) -> WidgetMut<'t, dyn Widget> {
        this.ctx.get_mut(&mut this.widget.child)
    }
}

impl HasProperty<HoveredBackground> for WindowControl {}
impl HasProperty<CornerRadius> for WindowControl {}
impl HasProperty<Padding> for WindowControl {}

impl Widget for WindowControl {
    type Action = HotPress;

    fn on_pointer_event(&mut self, ctx: &mut EventCtx<'_>, _props: &mut PropertiesMut<'_>, event: &PointerEvent) {
        match event {
            PointerEvent::Down(..) => {
                ctx.capture_pointer();
                ctx.request_paint_only();
            }
            PointerEvent::Up(PointerButtonEvent { button: Some(PointerButton::Primary), .. }) => {
                if ctx.is_active() && ctx.is_hovered() {
                    match self.kind {
                        WindowControlKind::Minimize => ctx.minimize(),
                        WindowControlKind::Maximize => ctx.toggle_maximized(),
                        WindowControlKind::Close => ctx.exit(),
                    }
                    ctx.set_handled();
                }
                ctx.request_paint_only();
            }
            _ => (),
        }
    }

    fn update(&mut self, ctx: &mut UpdateCtx<'_>, _props: &mut PropertiesMut<'_>, event: &Update) {
        if matches!(event, Update::HoveredChanged(_) | Update::ActiveChanged(_)) {
            ctx.request_paint_only();
        }
    }

    fn property_changed(&mut self, ctx: &mut UpdateCtx<'_>, property_type: TypeId) {
        HoveredBackground::prop_changed(ctx, property_type);
        CornerRadius::prop_changed(ctx, property_type);
        Padding::prop_changed(ctx, property_type);
    }

    fn register_children(&mut self, ctx: &mut RegisterCtx<'_>) {
        ctx.register_child(&mut self.child);
    }

    fn layout(&mut self, ctx: &mut LayoutCtx<'_>, props: &mut PropertiesMut<'_>, bc: &BoxConstraints) -> Size {
        let padding = props.get::<Padding>();
        let child_bc = padding.layout_down(bc.loosen());
        let child_size = ctx.run_layout(&mut self.child, &child_bc);
        let (size, _b) = padding.layout_up(child_size, 0.0);
        let size = bc.constrain(size);
        let offset = (size.to_vec2() - child_size.to_vec2()) / 2.0;
        ctx.place_child(&mut self.child, offset.to_point());
        size
    }

    fn paint(&mut self, ctx: &mut PaintCtx<'_>, props: &PropertiesRef<'_>, scene: &mut Scene) {
        if ctx.is_hovered() {
            let corner = props.get::<CornerRadius>().radius;
            let bg = &props.get::<HoveredBackground>().0;
            let rect = ctx.size().to_rect().to_rounded_rect(corner);
            let brush = bg.get_peniko_brush_for_rect(rect.rect());
            scene.fill(Fill::NonZero, Affine::IDENTITY, &brush, None, &rect);
        }
    }

    fn accessibility_role(&self) -> Role {
        Role::Button
    }

    fn accessibility(&mut self, _ctx: &mut AccessCtx<'_>, _props: &PropertiesRef<'_>, node: &mut Node) {
        node.add_action(accesskit::Action::Click);
    }

    fn children_ids(&self) -> ChildrenIds {
        ChildrenIds::from_slice(&[self.child.id()])
    }

    fn make_trace_span(&self, id: WidgetId) -> Span {
        trace_span!("WindowControl", id = id.trace())
    }
}

pub struct WindowControlView<Child> {
    child: Child,
    kind: WindowControlKind,
}

pub fn window_control<Child, State, Action>(
    kind: WindowControlKind,
    child: Child,
) -> WindowControlView<Child>
where
    Child: WidgetView<State, Action>,
{
    WindowControlView { child, kind }
}

impl<Child> ViewMarker for WindowControlView<Child> {}

impl<Child, State, Action> View<State, Action, ViewCtx> for WindowControlView<Child>
where
    State: 'static,
    Action: 'static,
    Child: WidgetView<State, Action>,
{
    type Element = Pod<WindowControl>;
    type ViewState = Child::ViewState;

    fn build(&self, ctx: &mut ViewCtx, app_state: &mut State) -> (Self::Element, Self::ViewState) {
        let (child_pod, child_state) =
            ctx.with_id(CHILD_VIEW_ID, |ctx| self.child.build(ctx, app_state));
        (ctx.create_pod(WindowControl::new(child_pod.new_widget.erased(), self.kind)), child_state)
    }

    fn rebuild(&self, prev: &Self, view_state: &mut Self::ViewState, ctx: &mut ViewCtx, mut element: Mut<'_, Self::Element>, app_state: &mut State) {
        ctx.with_id(CHILD_VIEW_ID, |ctx| {
            self.child.rebuild(
                &prev.child,
                view_state,
                ctx,
                WindowControl::child_mut(&mut element).downcast(),
                app_state,
            );
        });
    }

    fn teardown(&self, view_state: &mut Self::ViewState, ctx: &mut ViewCtx, mut element: Mut<'_, Self::Element>) {
        ctx.with_id(CHILD_VIEW_ID, |ctx| {
            self.child.teardown(view_state, ctx, WindowControl::child_mut(&mut element).downcast());
        });
    }

    fn message(&self, view_state: &mut Self::ViewState, message: &mut MessageContext, mut element: Mut<'_, Self::Element>, app_state: &mut State) -> MessageResult<Action> {
        self.child.message(
            view_state,
            message,
            WindowControl::child_mut(&mut element).downcast(),
            app_state,
        )
    }
}

// ---------------------------------------------------------------------------
// --- Glyph: vector icon widget ----------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Play,
    Pause,
    Prev,
    Next,
    Shuffle,
    Repeat,
    RepeatOne,
    Heart,
    HeartFilled,
    QueueList,
    VolumeHigh,
    VolumeMute,
    Minus,
    MaxSquare,
    XClose,
    ChevronLeft,
}

pub struct Glyph {
    icon: Icon,
    size: f64,
    color: Color,
}

impl Glyph {
    pub fn new(icon: Icon, size: f64, color: Color) -> Self {
        Self { icon, size, color }
    }
}

impl Widget for Glyph {
    type Action = HotPress;

    fn on_pointer_event(&mut self, _ctx: &mut EventCtx<'_>, _props: &mut PropertiesMut<'_>, _event: &PointerEvent) {}
    fn on_text_event(&mut self, _ctx: &mut EventCtx<'_>, _props: &mut PropertiesMut<'_>, _event: &TextEvent) {}
    fn on_access_event(&mut self, _ctx: &mut EventCtx<'_>, _props: &mut PropertiesMut<'_>, _event: &AccessEvent) {}

    fn register_children(&mut self, _ctx: &mut RegisterCtx<'_>) {}

    fn layout(&mut self, _ctx: &mut LayoutCtx<'_>, _props: &mut PropertiesMut<'_>, bc: &BoxConstraints) -> Size {
        bc.constrain(Size::new(self.size, self.size))
    }

    fn paint(&mut self, ctx: &mut PaintCtx<'_>, _props: &PropertiesRef<'_>, scene: &mut Scene) {
        let size = ctx.size();
        let s = self.size.min(size.width).min(size.height);
        // Icons are authored in a 24x24 box, scaled to `s`.
        let k = s / 24.0;
        let (ox, oy) = ((size.width - s) / 2.0, (size.height - s) / 2.0);
        let t = |x: f64, y: f64| Point::new(ox + x * k, oy + y * k);
        let color = self.color;
        let fill_shape = |scene: &mut Scene, path: BezPath| {
            scene.fill(Fill::NonZero, Affine::IDENTITY, color, None, &path);
        };
        let stroke_line = |scene: &mut Scene, from: (f64, f64), to: (f64, f64), w: f64| {
            let mut p = BezPath::new();
            p.move_to(t(from.0, from.1));
            p.line_to(t(to.0, to.1));
            scene.stroke(&Stroke::new(w * k), Affine::IDENTITY, color, None, &p);
        };

        match self.icon {
            Icon::Play => {
                let mut path = BezPath::new();
                path.move_to(t(7.0, 5.0));
                path.line_to(t(19.0, 12.0));
                path.line_to(t(7.0, 19.0));
                path.close_path();
                fill_shape(scene, path);
            }
            Icon::Pause => {
                for x in [7.0, 14.5] {
                    let mut r = BezPath::new();
                    r.move_to(t(x, 5.0));
                    r.line_to(t(x + 3.0, 5.0));
                    r.line_to(t(x + 3.0, 19.0));
                    r.line_to(t(x, 19.0));
                    r.close_path();
                    fill_shape(scene, r);
                }
            }
            Icon::Prev => {
                let mut path = BezPath::new();
                path.move_to(t(6.0, 5.0));
                path.line_to(t(10.0, 5.0));
                path.line_to(t(10.0, 19.0));
                path.line_to(t(6.0, 19.0));
                path.close_path();
                fill_shape(scene, path);
                let mut path = BezPath::new();
                path.move_to(t(18.0, 5.0));
                path.line_to(t(18.0, 19.0));
                path.line_to(t(9.5, 12.0));
                path.close_path();
                fill_shape(scene, path);
            }
            Icon::Next => {
                let mut path = BezPath::new();
                path.move_to(t(18.0, 5.0));
                path.line_to(t(14.0, 5.0));
                path.line_to(t(14.0, 19.0));
                path.line_to(t(18.0, 19.0));
                path.close_path();
                fill_shape(scene, path);
                let mut path = BezPath::new();
                path.move_to(t(6.0, 5.0));
                path.line_to(t(6.0, 19.0));
                path.line_to(t(14.5, 12.0));
                path.close_path();
                fill_shape(scene, path);
            }
            Icon::Shuffle => {
                stroke_line(scene, (3.5, 6.5), (20.5, 17.5), 2.0);
                stroke_line(scene, (3.5, 17.5), (20.5, 6.5), 2.0);
                let mut p = BezPath::new();
                p.move_to(t(20.5, 17.5));
                p.line_to(t(16.2, 16.6));
                p.line_to(t(19.8, 13.2));
                p.close_path();
                fill_shape(scene, p);
                let mut p = BezPath::new();
                p.move_to(t(20.5, 6.5));
                p.line_to(t(16.2, 7.4));
                p.line_to(t(19.8, 10.8));
                p.close_path();
                fill_shape(scene, p);
            }
            Icon::Repeat | Icon::RepeatOne => {
                stroke_line(scene, (7.0, 8.0), (17.0, 8.0), 2.0);
                stroke_line(scene, (17.0, 8.0), (17.0, 15.0), 2.0);
                stroke_line(scene, (17.0, 15.0), (7.0, 15.0), 2.0);
                stroke_line(scene, (7.0, 15.0), (7.0, 8.0), 2.0);
                let mut p = BezPath::new();
                p.move_to(t(17.0, 5.5));
                p.line_to(t(14.2, 9.5));
                p.line_to(t(19.8, 9.5));
                p.close_path();
                fill_shape(scene, p);
                if self.icon == Icon::RepeatOne {
                    stroke_line(scene, (12.0, 10.5), (12.0, 13.5), 2.0);
                }
            }
            Icon::Heart | Icon::HeartFilled => {
                let mut path = BezPath::new();
                path.move_to(t(12.0, 21.0));
                path.curve_to(t(4.0, 15.0), t(2.0, 9.5), t(6.0, 6.5));
                path.curve_to(t(9.0, 4.5), t(11.5, 6.0), t(12.0, 8.5));
                path.curve_to(t(12.5, 6.0), t(15.0, 4.5), t(18.0, 6.5));
                path.curve_to(t(22.0, 9.5), t(20.0, 15.0), t(12.0, 21.0));
                path.close_path();
                if self.icon == Icon::HeartFilled {
                    fill_shape(scene, path);
                } else {
                    scene.stroke(&Stroke::new(1.8 * k), Affine::IDENTITY, color, None, &path);
                }
            }
            Icon::QueueList => {
                stroke_line(scene, (4.0, 6.0), (13.0, 6.0), 2.0);
                stroke_line(scene, (4.0, 12.0), (13.0, 12.0), 2.0);
                stroke_line(scene, (4.0, 18.0), (13.0, 18.0), 2.0);
                let mut p = BezPath::new();
                p.move_to(t(16.5, 6.0));
                p.line_to(t(16.5, 18.0));
                p.line_to(t(22.0, 12.0));
                p.close_path();
                fill_shape(scene, p);
            }
            Icon::VolumeHigh => {
                let mut path = BezPath::new();
                path.move_to(t(4.0, 9.0));
                path.line_to(t(8.0, 9.0));
                path.line_to(t(13.0, 4.5));
                path.line_to(t(13.0, 19.5));
                path.line_to(t(8.0, 15.0));
                path.line_to(t(4.0, 15.0));
                path.close_path();
                fill_shape(scene, path);
                stroke_line(scene, (16.0, 9.0), (16.0, 15.0), 2.0);
                stroke_line(scene, (19.5, 6.5), (19.5, 17.5), 2.0);
            }
            Icon::VolumeMute => {
                let mut path = BezPath::new();
                path.move_to(t(4.0, 9.0));
                path.line_to(t(8.0, 9.0));
                path.line_to(t(13.0, 4.5));
                path.line_to(t(13.0, 19.5));
                path.line_to(t(8.0, 15.0));
                path.line_to(t(4.0, 15.0));
                path.close_path();
                fill_shape(scene, path);
                stroke_line(scene, (16.5, 8.5), (21.5, 15.5), 2.0);
                stroke_line(scene, (21.5, 8.5), (16.5, 15.5), 2.0);
            }
            Icon::Minus => {
                stroke_line(scene, (5.0, 12.0), (19.0, 12.0), 1.8);
            }
            Icon::MaxSquare => {
                let r = Rect::new(ox + 6.0 * k, oy + 6.0 * k, ox + 18.0 * k, oy + 18.0 * k);
                scene.stroke(&Stroke::new(1.8 * k), Affine::IDENTITY, color, None, &r);
            }
            Icon::XClose => {
                stroke_line(scene, (6.5, 6.5), (17.5, 17.5), 1.8);
                stroke_line(scene, (17.5, 6.5), (6.5, 17.5), 1.8);
            }
            Icon::ChevronLeft => {
                stroke_line(scene, (14.0, 5.0), (8.0, 12.0), 2.2);
                stroke_line(scene, (8.0, 12.0), (14.0, 19.0), 2.2);
            }
        }
    }

    fn accessibility_role(&self) -> Role {
        Role::Image
    }

    fn accessibility(&mut self, _ctx: &mut AccessCtx<'_>, _props: &PropertiesRef<'_>, node: &mut Node) {
        node.set_label(format!("{:?}", self.icon));
    }

    fn children_ids(&self) -> ChildrenIds {
        ChildrenIds::new()
    }

    fn make_trace_span(&self, id: WidgetId) -> Span {
        trace_span!("Glyph", id = id.trace())
    }
}

pub struct GlyphView {
    icon: Icon,
    size: f64,
    color: Color,
}

pub fn glyph(icon: Icon, size: f64, color: Color) -> GlyphView {
    GlyphView { icon, size, color }
}

impl ViewMarker for GlyphView {}

impl<State, Action> View<State, Action, ViewCtx> for GlyphView {
    type Element = Pod<Glyph>;
    type ViewState = ();

    fn build(&self, ctx: &mut ViewCtx, _app_state: &mut State) -> (Self::Element, Self::ViewState) {
        (ctx.create_pod(Glyph::new(self.icon, self.size, self.color)), ())
    }

    fn rebuild(&self, prev: &Self, (): &mut Self::ViewState, _ctx: &mut ViewCtx, mut element: Mut<'_, Self::Element>, _app_state: &mut State) {
        if prev.icon != self.icon || prev.size != self.size || prev.color != self.color {
            element.widget.icon = self.icon;
            element.widget.size = self.size;
            element.widget.color = self.color;
            element.ctx.request_render();
        }
    }

    fn teardown(&self, (): &mut Self::ViewState, _ctx: &mut ViewCtx, _element: Mut<'_, Self::Element>) {}

    fn message(&self, (): &mut Self::ViewState, _message: &mut MessageContext, _element: Mut<'_, Self::Element>, _app_state: &mut State) -> MessageResult<Action> {
        MessageResult::Stale
    }
}

// ---------------------------------------------------------------------------
// --- Scrolled: a portal whose scroll position is driven by state ------------
// The bench needs "jump to fraction f of the list, no easing" every frame;
// xilem 0.4's portal view has no state->widget scroll path, so this custom
// view reads the fraction and calls `Portal::set_viewport_pos` on rebuild.

pub struct Scrolled<Child> {
    child: Child,
    frac: Option<f64>,
}

pub fn scrolled<Child>(frac: Option<f64>, child: Child) -> Scrolled<Child> {
    Scrolled { child, frac }
}

impl<Child> ViewMarker for Scrolled<Child> {}

impl<Child, State, Action> View<State, Action, ViewCtx> for Scrolled<Child>
where
    State: 'static,
    Action: 'static,
    Child: WidgetView<State, Action>,
{
    type Element = Pod<masonry::widgets::Portal<Child::Widget>>;
    type ViewState = Child::ViewState;

    fn build(&self, ctx: &mut ViewCtx, app_state: &mut State) -> (Self::Element, Self::ViewState) {
        let (child_pod, child_state) = self.child.build(ctx, app_state);
        (ctx.create_pod(masonry::widgets::Portal::new(child_pod.new_widget)), child_state)
    }

    fn rebuild(&self, prev: &Self, view_state: &mut Self::ViewState, ctx: &mut ViewCtx, mut element: Mut<'_, Self::Element>, app_state: &mut State) {
        // Read the current geometry (from the last layout) before mutably
        // rebuilding the child.
        let portal_size = element.ctx.size();
        let content_size = masonry::widgets::Portal::child_mut(&mut element).ctx.size();
        self.child.rebuild(
            &prev.child,
            view_state,
            ctx,
            masonry::widgets::Portal::child_mut(&mut element).downcast(),
            app_state,
        );
        if self.frac != prev.frac {
            if let Some(f) = self.frac {
                let max_y = (content_size.height - portal_size.height).max(0.0);
                let y = f * max_y;
                masonry::widgets::Portal::set_viewport_pos(&mut element, Point::new(0.0, y));
            }
        }
    }

    fn teardown(&self, view_state: &mut Self::ViewState, ctx: &mut ViewCtx, mut element: Mut<'_, Self::Element>) {
        let child = masonry::widgets::Portal::child_mut(&mut element);
        self.child.teardown(view_state, ctx, child);
    }

    fn message(&self, view_state: &mut Self::ViewState, message: &mut MessageContext, mut element: Mut<'_, Self::Element>, app_state: &mut State) -> MessageResult<Action> {
        let child = masonry::widgets::Portal::child_mut(&mut element);
        self.child.message(view_state, message, child, app_state)
    }
}
