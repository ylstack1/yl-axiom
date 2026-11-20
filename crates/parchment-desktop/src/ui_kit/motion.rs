use super::*;
/// Underdamped interactive press/release bounce spring matching Kotlin `Motion.SCALE_SPRING`
/// (`stiffness = 540`, `dampingRatio = 0.52`, ~15% tactile overshoot).
pub const SPRING_BOUNCE: SpringConfig = SpringConfig::new(540.0, 24.2, 1.0);
/// Pop-in spring for floating popovers, drawers, modals, and pills (`Motion.POP_STIFFNESS`).
pub const SPRING_POP: SpringConfig = SpringConfig::new(580.0, 31.0, 1.0);
/// Calm spring for collapsible sidebars and sheets.
pub const SPRING_PANEL: SpringConfig = SpringConfig::new(340.0, 30.0, 1.0);

/// Breathing activity dot matching `KissStreamingPulseDot` (`750ms` pulse, auto-static under Reduce Motion).
pub fn streaming_pulse_dot(id: impl Into<ElementId>, color: Hsla) -> AnyElement {
    div()
        .w(px(7.0))
        .h(px(7.0))
        .rounded(px(999.0))
        .bg(color)
        .flex_shrink_0()
        .with_animation(
            id,
            Animation::new(Duration::from_millis(750))
                .repeat()
                .with_easing(pulsating_between(0.35, 1.0)),
            |el, alpha| el.opacity(alpha),
        )
        .into_any_element()
}

/// Smooth pop-in entrance (clamped opacity + underdamped vertical spring settle) honoring `App::reduce_motion`.
pub fn spring_pop_enter(id: impl Into<ElementId>, child: impl IntoElement + 'static) -> AnyElement {
    div()
        .child(child)
        .with_spring(
            id,
            SpringAnimation::new(SPRING_POP).to(1.0_f32).from(0.0_f32),
            |el, t| {
                let alpha = t.clamp(0.0, 1.0);
                let offset = ((1.0 - t) * 7.0).clamp(-3.0, 10.0);
                el.opacity(alpha).mt(px(offset))
            },
        )
        .into_any_element()
}

#[derive(Clone, Copy, Default)]
pub struct BounceFlags {
    pressed: bool,
}

struct KissBounceState {
    spring: SpringState,
    flags: Rc<Cell<BounceFlags>>,
    updated_at: Instant,
}

/// Zero-layout-shift interactive spring wrapper mirroring Kotlin `KissMotionNode`:
/// stays rock-steady on hover (no vertical lift or border clipping inside `overflow_hidden()`)
/// and applies a subtle tactile press settle for buttons/chips (0px for list rows/tabs,
/// instantly disabled when `App::reduce_motion()` is active; 0 idle frames at rest).
pub struct KissBounce<E> {
    id: ElementId,
    element: Option<E>,
    amplitude_px: f32,
    enabled: bool,
}

/// Subtle tactile press settle for buttons, icon buttons, chips, and compact cards.
pub fn kiss_bounce<E: IntoElement + 'static>(id: impl Into<ElementId>, child: E) -> KissBounce<E> {
    KissBounce {
        id: id.into(),
        element: Some(child),
        amplitude_px: 1.0,
        enabled: true,
    }
}

/// Calm zero-shift wrapper for wide list rows (sidebar items, tabs, model/settings rows).
pub fn kiss_bounce_subtle<E: IntoElement + 'static>(
    id: impl Into<ElementId>,
    child: E,
) -> KissBounce<E> {
    KissBounce {
        id: id.into(),
        element: Some(child),
        amplitude_px: 0.0,
        enabled: true,
    }
}

impl<E> KissBounce<E> {
    pub fn map_element(mut self, f: impl FnOnce(E) -> E) -> Self {
        self.element = self.element.map(f);
        self
    }
}

impl<E: gpui::Styled> KissBounce<E> {
    pub fn w_full(self) -> Self {
        self.map_element(|el| el.w_full())
    }
    pub fn flex_1(self) -> Self {
        self.map_element(|el| el.flex_1())
    }
    pub fn h(self, height: impl Into<Length>) -> Self {
        let h = height.into();
        self.map_element(move |el| el.h(h))
    }
    pub fn px(self, pad: impl Into<gpui::DefiniteLength>) -> Self {
        let p = pad.into();
        self.map_element(move |el| el.px(p))
    }
    pub fn justify_start(self) -> Self {
        self.map_element(|el| el.justify_start())
    }
    pub fn border_color(self, color: impl Into<Hsla>) -> Self {
        let c = color.into();
        self.map_element(move |el| el.border_color(c))
    }
    pub fn text_color(self, color: impl Into<Hsla>) -> Self {
        let c = color.into();
        self.map_element(move |el| el.text_color(c))
    }
}

impl<E: IntoElement + 'static> IntoElement for KissBounce<E> {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl<E: IntoElement + 'static> Element for KissBounce<E> {
    type RequestLayoutState = AnyElement;
    type PrepaintState = Option<(Hitbox, Rc<Cell<BounceFlags>>)>;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }
    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut child = match self.element.take() {
            Some(el) => el.into_any_element(),
            None => div().into_any_element(),
        };
        let layout_id = child.request_layout(window, cx);
        (layout_id, child)
    }
    fn prepaint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        child: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        if !self.enabled || self.amplitude_px <= 0.0 {
            child.prepaint(window, cx);
            return None;
        }
        let Some(gid) = global_id else {
            child.prepaint(window, cx);
            return None;
        };
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        let reduce_motion = cx.reduce_motion();
        let amp = self.amplitude_px;
        let (y_offset, flags_rc) = window.with_element_state(gid, |prev, window| {
            let now = Instant::now();
            let mut st = prev.unwrap_or_else(|| KissBounceState {
                spring: SpringState::default(),
                flags: Rc::new(Cell::new(BounceFlags::default())),
                updated_at: now,
            });
            let f = st.flags.get();
            if reduce_motion {
                st.spring = SpringState::default();
                st.updated_at = now;
                let rc = st.flags.clone();
                return ((0.0_f32, rc), st);
            }
            let target = if f.pressed { 1.0_f32 } else { 0.0_f32 };
            let dt = now
                .duration_since(st.updated_at)
                .as_secs_f32()
                .clamp(0.0, 0.05);
            st.spring = SPRING_BOUNCE.step(st.spring, target, dt);
            st.updated_at = now;
            if SPRING_BOUNCE.is_settled(st.spring, target, 0.004) {
                st.spring = SpringState {
                    position: target,
                    velocity: 0.0,
                };
            } else {
                window.request_animation_frame();
            }
            let y = (st.spring.position * amp).clamp(0.0, amp);
            let rc = st.flags.clone();
            ((y, rc), st)
        });
        if y_offset.abs() > 0.01 {
            window.with_element_offset(point(px(0.0), px(y_offset)), |window| {
                child.prepaint(window, cx);
            });
        } else {
            child.prepaint(window, cx);
        }
        Some((hitbox, flags_rc))
    }
    fn paint(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        child: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        child.paint(window, cx);
        let Some((hitbox, flags)) = prepaint.clone() else {
            return;
        };
        let hb_down = hitbox.clone();
        let f_down = flags.clone();
        window.on_mouse_event(move |ev: &MouseDownEvent, phase, window, _cx| {
            if phase == DispatchPhase::Capture
                && ev.button == MouseButton::Left
                && hb_down.is_hovered(window)
            {
                let mut f = f_down.get();
                if !f.pressed {
                    f.pressed = true;
                    f_down.set(f);
                    window.refresh();
                }
            }
        });
        let f_up = flags.clone();
        window.on_mouse_event(move |ev: &MouseUpEvent, phase, window, _cx| {
            if phase == DispatchPhase::Capture && ev.button == MouseButton::Left {
                let mut f = f_up.get();
                if f.pressed {
                    f.pressed = false;
                    f_up.set(f);
                    window.refresh();
                }
            }
        });
    }
}
/// Smooth horizontal slide-in/out drawer (animates width `0..width_px` + opacity, honoring `reduce_motion`).
pub fn spring_slide_drawer(
    id: impl Into<ElementId>,
    open: bool,
    width_px: f32,
    child: impl IntoElement + 'static,
) -> AnyElement {
    div()
        .h_full()
        .flex_shrink_0()
        .overflow_hidden()
        .child(div().w(px(width_px)).h_full().flex_shrink_0().child(child))
        .with_spring(
            id,
            SpringAnimation::new(SPRING_PANEL).to(open),
            move |el, phase| {
                let t: f32 = phase.interpolate_clamped(0.0_f32, 1.0_f32);
                el.w(px((width_px * t).max(0.0))).opacity(t)
            },
        )
        .into_any_element()
}

pub(super) fn kiss_bounce_opt<E: IntoElement + 'static>(
    id: impl Into<ElementId>,
    child: E,
    amplitude_px: f32,
    enabled: bool,
) -> KissBounce<E> {
    KissBounce {
        id: id.into(),
        element: Some(child),
        amplitude_px,
        enabled,
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spring_bounce_matches_kotlin_underdamped_overshoot_and_settles() {
        let (_omega, zeta) = SPRING_BOUNCE.canonical();
        assert!(
            (zeta - 0.52).abs() < 0.02,
            "Expected damping ratio ~0.52 matching Kotlin SCALE_SPRING, got {zeta}"
        );
        let mut state = SpringState {
            position: 1.0,
            velocity: -18.0,
        };
        let mut min_pos = 1.0_f32;
        for _ in 0..40 {
            state = SPRING_BOUNCE.step(state, 0.0, 1.0 / 60.0);
            min_pos = min_pos.min(state.position);
        }
        assert!(
            min_pos < -0.10,
            "Expected tactile bounce overshoot < -0.10, got {min_pos}"
        );
        assert!(
            SPRING_BOUNCE.is_settled(state, 0.0, 0.01),
            "Spring must settle smoothly within 0.67s, got pos={} vel={}",
            state.position,
            state.velocity
        );
    }
}
