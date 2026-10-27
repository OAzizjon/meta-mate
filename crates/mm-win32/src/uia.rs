//! UI Automation snapshots from Rust (spike S3): one cached, bounded walk of a window's
//! control view. Fail closed (invariant 3): any walk error or a hit limit marks the
//! snapshot truncated, and a truncated snapshot blocks the screenshot (`redaction`).
//!
//! The walker runs on the calling thread, which `UiaWalker::new` puts into the COM
//! multithreaded apartment; keep it off the UI thread (a hung provider must never
//! freeze Mate).

use mm_core::geom::PhysRect;
use mm_core::privacy::password_like;
use mm_platform::{TargetWindow, UiElement, UiSnapshot, is_interactive_type};
use uiautomation::core::UICacheRequest;
use uiautomation::types::{Handle, TreeScope, UIProperty};
use uiautomation::{UIAutomation, UIElement, UITreeWalker};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

/// Limits of one walk (docs/architecture.md section 2: about 400 elements).
#[derive(Debug, Clone, Copy)]
pub struct WalkLimits {
    pub max_nodes: usize,
    pub max_depth: usize,
}

impl Default for WalkLimits {
    fn default() -> Self {
        Self {
            max_nodes: 400,
            max_depth: 12,
        }
    }
}

/// Every property a snapshot reads, fetched in one round trip per element.
const PROPERTIES: [UIProperty; 9] = [
    UIProperty::ControlType,
    UIProperty::Name,
    UIProperty::AutomationId,
    UIProperty::ClassName,
    UIProperty::HelpText,
    UIProperty::BoundingRectangle,
    UIProperty::IsEnabled,
    UIProperty::IsOffscreen,
    UIProperty::IsPassword,
];

pub struct UiaWalker {
    automation: UIAutomation,
    cache: UICacheRequest,
    walker: UITreeWalker,
}

impl UiaWalker {
    /// Initializes COM (multithreaded) on this thread and prepares the cache request.
    pub fn new() -> uiautomation::Result<Self> {
        let automation = UIAutomation::new()?;
        let cache = automation.create_cache_request()?;
        for property in PROPERTIES {
            cache.add_property(property)?;
        }
        cache.set_tree_scope(TreeScope::Element)?;
        let walker = automation.get_control_view_walker()?;
        Ok(Self {
            automation,
            cache,
            walker,
        })
    }

    /// A snapshot of `hwnd`'s control view, pre-order, ids `e0`, `e1`, ... in walk
    /// order. Never reads control values (passwords stay where they are).
    pub fn snapshot(&self, hwnd: HWND, limits: WalkLimits) -> uiautomation::Result<UiSnapshot> {
        let root = self
            .automation
            .element_from_handle_build_cache(Handle::from(hwnd), &self.cache)?;
        let mut elements = Vec::new();
        let mut truncated = false;
        // (element, parent id, depth)
        let mut stack: Vec<(UIElement, Option<String>, usize)> = vec![(root, None, 0)];
        while let Some((element, parent, depth)) = stack.pop() {
            if elements.len() >= limits.max_nodes {
                truncated = true;
                break;
            }
            let id = format!("e{}", elements.len());
            match describe(&element, &id, parent) {
                Some(described) => elements.push(described),
                None => {
                    truncated = true;
                    continue;
                }
            }
            if depth + 1 > limits.max_depth {
                if self
                    .walker
                    .get_first_child_build_cache(&element, &self.cache)
                    .is_ok()
                {
                    truncated = true;
                }
                continue;
            }
            // Children are pushed in reverse so they pop in document order.
            let mut children = Vec::new();
            let mut next = self
                .walker
                .get_first_child_build_cache(&element, &self.cache)
                .ok();
            while let Some(child) = next {
                next = self
                    .walker
                    .get_next_sibling_build_cache(&child, &self.cache)
                    .ok();
                children.push(child);
                if children.len() > limits.max_nodes {
                    truncated = true;
                    break;
                }
            }
            for child in children.into_iter().rev() {
                stack.push((child, Some(id.clone()), depth + 1));
            }
        }
        let frame = elements
            .first()
            .and_then(|root: &UiElement| root.rect)
            .unwrap_or(PhysRect {
                left: 0,
                top: 0,
                right: 1,
                bottom: 1,
            });
        Ok(UiSnapshot {
            target: target_window(hwnd),
            frame,
            elements,
            truncated,
        })
    }
}

/// One element's cached properties; `None` when the provider failed to answer.
fn describe(element: &UIElement, id: &str, parent: Option<String>) -> Option<UiElement> {
    let control_type = format!("{:?}", element.get_cached_control_type().ok()?);
    let name = element.get_cached_name().unwrap_or_default();
    let automation_id = element.get_cached_automation_id().unwrap_or_default();
    let class_name = element.get_cached_classname().unwrap_or_default();
    let help_text = element.get_cached_help_text().unwrap_or_default();
    let rect = element
        .get_cached_bounding_rectangle()
        .ok()
        .and_then(|r| PhysRect::new(r.get_left(), r.get_top(), r.get_right(), r.get_bottom()).ok());
    // Unknown flags fail closed, like the replay backend.
    let is_password = element.is_cached_password().unwrap_or(true);
    let enabled = element.is_cached_enabled().unwrap_or(false);
    let offscreen = element.is_cached_offscreen().unwrap_or(true);
    Some(UiElement {
        id: id.to_owned(),
        parent,
        interactive: is_interactive_type(&control_type),
        password_like: password_like(
            &control_type,
            &name,
            &automation_id,
            &help_text,
            &class_name,
        ),
        control_type,
        name,
        rect,
        enabled,
        offscreen,
        is_password,
    })
}

fn target_window(hwnd: HWND) -> TargetWindow {
    let mut process_id = 0u32;
    // SAFETY: the out pointer is a local u32 that outlives the call.
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut process_id)) };
    TargetWindow {
        handle: hwnd.0 as usize as u64,
        process_id,
    }
}
