//! Which webcams can be recorded beside a screen (F-22): enumeration through
//! Media Foundation's `MFEnumDeviceSources` with
//! `MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID`, and the id a device is
//! addressed by across IPC.
//!
//! Split from `source.rs` (644 of the 800-line Rust cap) the way
//! `source_derive` was, and re-exported from it, so `source::list_webcams`
//! is the path every caller uses.
//!
//! The id is `webcam:<sha256 of the lower-cased symbolic link>`: the link
//! itself (`\\?\usb#vid_046d&pid_085c#...#{e5323777-...}\global`) is neither
//! stable to print nor safe to hand a webview, and a list index is not
//! stable across a re-enumeration. `WebcamDeviceId::parse` (Task 51) is the
//! strict gate on the way back in.
//!
//! Enumeration DEGRADES to an empty list rather than erroring, the
//! `list_sources` precedent: "no webcam" is an ordinary state the picker
//! renders, a WinRT/COM error is not something the user can act on.

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::source::WebcamDeviceId;

/// One webcam in the picker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureWebcamInfo {
    /// `webcam:<hash>` — what `start_screen_capture`'s `webcamId` carries.
    pub id: String,
    /// The device's friendly name, as Windows shows it.
    pub label: String,
}

/// The id a device with this Media Foundation symbolic link is addressed by.
/// Lower-cased first: the same device's link has been seen spelled with
/// different case by different enumerations, and two ids for one camera
/// would make a pick fail to resolve.
pub fn webcam_id_for_link(link: &str) -> WebcamDeviceId {
    let digest = Sha256::digest(link.to_lowercase().as_bytes());
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    WebcamDeviceId::parse(&format!("webcam:{hex}"))
        .expect("a sha256 in lowercase hex is a canonical webcam id")
}

#[cfg(not(windows))]
mod imp {
    use super::*;

    pub fn list_webcams() -> Vec<CaptureWebcamInfo> {
        log::debug!("screen source: webcam enumeration is Windows-only; returning an empty list");
        Vec::new()
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use windows::core::{GUID, PWSTR};
    use windows::Win32::Media::MediaFoundation::{
        IMFActivate, IMFAttributes, MFCreateAttributes, MFEnumDeviceSources,
        MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME, MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
        MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
        MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK,
    };
    use windows::Win32::System::Com::{
        CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_MULTITHREADED,
    };

    /// COM on the calling thread for as long as this lives. A thread that
    /// already joined a DIFFERENT apartment answers `RPC_E_CHANGED_MODE`,
    /// which still lets Media Foundation run — so it is used, not refused,
    /// and not balanced with an uninitialize it does not own.
    pub(crate) struct ComScope {
        owned: bool,
    }

    impl ComScope {
        pub(crate) fn enter() -> ComScope {
            let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            if hr.is_err() {
                log::debug!("screen source: COM was already initialized differently ({hr:?})");
            }
            ComScope { owned: hr.is_ok() }
        }
    }

    impl Drop for ComScope {
        fn drop(&mut self) {
            if self.owned {
                unsafe { CoUninitialize() };
            }
        }
    }

    /// One enumerated video-capture device.
    pub(crate) struct Device {
        pub activate: IMFActivate,
        pub label: String,
        pub id: WebcamDeviceId,
    }

    fn string_attr(attrs: &IMFAttributes, key: &GUID) -> Option<String> {
        let mut value = PWSTR::null();
        let mut len = 0u32;
        unsafe {
            attrs.GetAllocatedString(key, &mut value, &mut len).ok()?;
            let text = value.to_string().ok();
            CoTaskMemFree(Some(value.0 as *const _));
            text
        }
    }

    /// Every video-capture device Media Foundation lists, skipping (and
    /// logging) one it cannot read rather than emptying the whole list.
    pub(crate) fn enumerate() -> Vec<Device> {
        let mut out = Vec::new();
        unsafe {
            let mut attrs: Option<IMFAttributes> = None;
            if let Err(e) = MFCreateAttributes(&mut attrs, 1) {
                log::warn!("screen source: cannot build the webcam query: {e}");
                return out;
            }
            let Some(attrs) = attrs else { return out };
            if let Err(e) = attrs.SetGUID(
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
            ) {
                log::warn!("screen source: cannot build the webcam query: {e}");
                return out;
            }
            let mut devices: *mut Option<IMFActivate> = std::ptr::null_mut();
            let mut count = 0u32;
            if let Err(e) = MFEnumDeviceSources(&attrs, &mut devices, &mut count) {
                log::warn!("screen source: webcam enumeration failed: {e}");
                return out;
            }
            if devices.is_null() {
                return out;
            }
            // The array is CoTaskMem-allocated and owns one reference per
            // entry: take each out (dropping it releases it), then free the
            // array itself.
            for i in 0..count as usize {
                let Some(activate) = std::ptr::replace(devices.add(i), None) else {
                    continue;
                };
                let link = string_attr(
                    &activate,
                    &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK,
                );
                let Some(link) = link else {
                    log::warn!("screen source: skipping a webcam with no symbolic link");
                    continue;
                };
                let label = string_attr(&activate, &MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME)
                    .filter(|l| !l.trim().is_empty())
                    .unwrap_or_else(|| "Webcam".to_string());
                out.push(Device {
                    activate,
                    label,
                    id: webcam_id_for_link(&link),
                });
            }
            CoTaskMemFree(Some(devices as *const _));
        }
        out
    }

    pub fn list_webcams() -> Vec<CaptureWebcamInfo> {
        let _com = ComScope::enter();
        enumerate()
            .into_iter()
            .map(|d| CaptureWebcamInfo {
                id: d.id.to_string(),
                label: d.label,
            })
            .collect()
    }
}

pub use imp::list_webcams;
#[cfg(windows)]
pub(crate) use imp::{enumerate, ComScope};

#[cfg(test)]
mod tests {
    use super::*;

    const LINK: &str = r"\\?\usb#vid_046d&pid_085c&mi_00#6&2a1b7f3&0&0000#{e5323777-f976-4f5b-9b55-b94699c46e44}\global";

    // The id round-trips through the strict parser the start path uses, so a
    // listed device can always be picked; the same device always gets the
    // same id, whatever case the link was spelled in; two devices never
    // share one.
    #[test]
    fn a_webcam_id_is_a_stable_canonical_hash_of_its_symbolic_link() {
        let id = webcam_id_for_link(LINK);
        assert_eq!(WebcamDeviceId::parse(&id.to_string()), Some(id.clone()));
        assert_eq!(id.hash().len(), 64);
        assert_eq!(webcam_id_for_link(LINK), id);
        assert_eq!(webcam_id_for_link(&LINK.to_uppercase()), id);
        assert_ne!(webcam_id_for_link(&LINK.replace("085c", "0825")), id);
        assert!(
            !id.to_string().contains("usb#"),
            "the link is never exposed"
        );
    }

    // A literal, not a struct re-serialized against itself: a snake_case
    // field would reach the picker as `undefined` with no error.
    #[test]
    fn a_webcam_row_crosses_the_wire_in_camel_case() {
        let row = CaptureWebcamInfo {
            id: "webcam:0f3a".into(),
            label: "Integrated Camera".into(),
        };
        assert_eq!(
            serde_json::to_string(&row).unwrap(),
            r#"{"id":"webcam:0f3a","label":"Integrated Camera"}"#
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn listing_webcams_off_windows_degrades_to_empty() {
        assert!(list_webcams().is_empty());
    }

    // Shape, not contents: a CI runner has no camera, a developer machine
    // may have several. Whatever comes back must be pickable.
    #[cfg(windows)]
    #[test]
    fn listing_webcams_on_windows_never_panics_and_every_id_parses() {
        for cam in list_webcams() {
            assert!(!cam.label.is_empty());
            assert!(WebcamDeviceId::parse(&cam.id).is_some(), "{}", cam.id);
        }
    }
}
