//! Minimal Rust binding of ERNativeUI's C ABI (include/ernativeui/erui.h,
//! API 1.3), plus the NativeTweaks fork's live-text exports. Only what the
//! NPC Spawner tab uses; layouts are checked against the published sizes.

#![allow(non_camel_case_types, dead_code)]

use std::ffi::c_void;

pub type Result = u32;
pub const OK: Result = 0;
pub const HOST_NOT_READY: Result = 2;

pub type ProviderHandle = u64;
pub type PageHandle = u64;
pub type RowHandle = u64;

pub const fn version(major: u32, minor: u32) -> u32 {
    (major << 16) | (minor & 0xFFFF)
}
pub const API_VERSION_1_3: u32 = version(1, 3);
pub const API_V1_3_SIZE: u32 = 384;

pub const CAP_TEXT_INPUT: u64 = 1 << 11;
pub const CAP_NUMERIC_SLIDER: u64 = 1 << 16;
pub const CAP_CUSTOM_SLIDER: u64 = 1 << 17;
pub const CAP_CUSTOM_TABS: u64 = 1 << 18;

pub const TAB_ICON_FORMAT_RGBA8: u32 = 1;
pub const TEXT_INPUT_MAX_LENGTH: u32 = 35;
pub const TAB_ICON_BYTES: usize = 32 * 32 * 4;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct StringView {
    pub data: *const u8,
    pub length: u32,
    pub reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Utf16View {
    pub data: *const u16,
    pub length: u32,
    pub reserved: u32,
}

impl StringView {
    pub fn new(s: &'static str) -> Self {
        StringView { data: s.as_ptr(), length: s.len() as u32, reserved: 0 }
    }
}

impl Utf16View {
    /// The slice must stay alive for the call (the host copies it).
    pub fn new(s: &[u16]) -> Self {
        Utf16View { data: s.as_ptr(), length: s.len() as u32, reserved: 0 }
    }
    pub const EMPTY: Utf16View = Utf16View { data: std::ptr::null(), length: 0, reserved: 0 };
}

pub fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

pub type ButtonCallback = extern "C" fn(user_data: *mut c_void);
pub type ValueChangedCallback = extern "C" fn(user_data: *mut c_void, value: u8);
pub type TextInputChangedCallback = extern "C" fn(user_data: *mut c_void, context: *const TextInputChangeContext);
pub type CustomSliderChangedCallback =
    extern "C" fn(user_data: *mut c_void, context: *const CustomSliderChangeContext);
pub type NumericSliderChangedCallback =
    extern "C" fn(user_data: *mut c_void, context: *const NumericSliderChangeContext);

#[repr(C)]
pub struct TextInputChangeContext {
    pub size: u32,
    pub flags: u32,
    pub provider: ProviderHandle,
    pub row: RowHandle,
    pub value: Utf16View,
    pub reserved: [u32; 2],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct DecimalValue {
    pub scaled_value: i64,
    pub decimal_places: u32,
    pub reserved: u32,
}

#[repr(C)]
pub struct NumericSliderChangeContext {
    pub size: u32,
    pub flags: u32,
    pub provider: ProviderHandle,
    pub row: RowHandle,
    pub value: DecimalValue,
    pub index: u32,
    pub value_count: u32,
    pub reserved: [u32; 2],
}

#[repr(C)]
pub struct CustomSliderChangeContext {
    pub size: u32,
    pub flags: u32,
    pub provider: ProviderHandle,
    pub row: RowHandle,
    pub index: u32,
    pub value_count: u32,
    pub reserved: [u32; 2],
}

#[repr(C)]
pub struct CustomSliderDesc {
    pub size: u32,
    pub flags: u32,
    pub label: Utf16View,
    pub help: Utf16View,
    pub changed_callback: Option<CustomSliderChangedCallback>,
    pub user_data: *mut c_void,
    pub values: *const Utf16View,
    pub value_count: u32,
    pub initial_index: u32,
    pub enabled: u32,
    pub reserved: u32,
}

#[repr(C)]
pub struct ProviderDesc {
    pub size: u32,
    pub api_version: u32,
    pub flags: u32,
    pub root_priority: i32,
    pub owner_module: *mut c_void,
    pub provider_id: StringView,
    pub display_name: Utf16View,
}

#[repr(C)]
pub struct TabIconDesc {
    pub size: u32,
    pub flags: u32,
    pub format: u32,
    pub byte_count: u32,
    pub pixels: *const u8,
    pub reserved: [u32; 2],
}

#[repr(C)]
pub struct TabDesc {
    pub size: u32,
    pub flags: u32,
    pub tab_id: StringView,
    pub title: Utf16View,
    pub preferred_order: i32,
    pub reserved0: u32,
    pub icon: *const TabIconDesc,
    pub reserved: [u32; 2],
}

#[repr(C)]
pub struct ButtonDesc {
    pub size: u32,
    pub flags: u32,
    pub label: Utf16View,
    pub help: Utf16View,
    pub callback: Option<ButtonCallback>,
    pub user_data: *mut c_void,
    pub enabled: u32,
    pub reserved: u32,
}

#[repr(C)]
pub struct ToggleDesc {
    pub size: u32,
    pub flags: u32,
    pub label: Utf16View,
    pub help: Utf16View,
    pub changed_callback: Option<ValueChangedCallback>,
    pub user_data: *mut c_void,
    pub initial_value: u8,
    pub reserved8: [u8; 3],
    pub enabled: u32,
}

#[repr(C)]
pub struct ChoiceDesc {
    pub size: u32,
    pub flags: u32,
    pub label: Utf16View,
    pub help: Utf16View,
    pub changed_callback: Option<ValueChangedCallback>,
    pub user_data: *mut c_void,
    pub options: *const Utf16View,
    pub option_count: u32,
    pub initial_index: u8,
    pub reserved8: [u8; 3],
}

#[repr(C)]
pub struct NumericSliderDesc {
    pub size: u32,
    pub flags: u32,
    pub label: Utf16View,
    pub help: Utf16View,
    pub changed_callback: Option<NumericSliderChangedCallback>,
    pub user_data: *mut c_void,
    pub minimum_scaled_value: i64,
    pub maximum_scaled_value: i64,
    pub step_scaled_value: i64,
    pub initial_scaled_value: i64,
    pub decimal_places: u32,
    pub enabled: u32,
    pub prefix: Utf16View,
    pub suffix: Utf16View,
    pub reserved: [u32; 2],
}

#[repr(C)]
pub struct TextInputDesc {
    pub size: u32,
    pub flags: u32,
    pub label: Utf16View,
    pub help: Utf16View,
    pub initial_value: Utf16View,
    pub placeholder: Utf16View,
    pub changed_callback: Option<TextInputChangedCallback>,
    pub user_data: *mut c_void,
    pub maximum_length: u32,
    pub reserved: u32,
}

#[repr(C)]
pub struct SubmenuDesc {
    pub size: u32,
    pub flags: u32,
    pub label: Utf16View,
    pub help: Utf16View,
    pub page_title: Utf16View,
    pub page_help: Utf16View,
    pub enabled: u32,
    pub reserved: u32,
}

type Slot = usize;

/// ERUI_Api (API 1.3). Entries this mod does not call are plain slots.
#[repr(C)]
pub struct Api {
    pub size: u32,
    pub api_version: u32,
    pub capabilities: u64,
    pub register_provider:
        Option<extern "C" fn(*const ProviderDesc, *mut ProviderHandle, *mut PageHandle) -> Result>,
    pub add_button: Option<extern "C" fn(ProviderHandle, PageHandle, *const ButtonDesc, *mut RowHandle) -> Result>,
    pub add_toggle: Option<extern "C" fn(ProviderHandle, PageHandle, *const ToggleDesc, *mut RowHandle) -> Result>,
    add_slider: Slot,
    pub add_inline_choice:
        Option<extern "C" fn(ProviderHandle, PageHandle, *const ChoiceDesc, *mut RowHandle) -> Result>,
    add_popup_choice: Slot,
    pub add_submenu: Option<
        extern "C" fn(ProviderHandle, PageHandle, *const SubmenuDesc, *mut PageHandle, *mut RowHandle) -> Result,
    >,
    set_page_presentation: Slot,
    pub commit_provider: Option<extern "C" fn(ProviderHandle) -> Result>,
    pub abort_provider: Option<extern "C" fn(ProviderHandle) -> Result>,
    pub set_row_value: Option<extern "C" fn(ProviderHandle, RowHandle, u8) -> Result>,
    get_row_value: Slot,
    enqueue_alert: Slot,
    get_game_language: Slot,
    // API 1.1
    pub add_text_input:
        Option<extern "C" fn(ProviderHandle, PageHandle, *const TextInputDesc, *mut RowHandle) -> Result>,
    pub set_text_input_value: Option<extern "C" fn(ProviderHandle, RowHandle, *const Utf16View) -> Result>,
    get_text_input_value: Slot,
    add_color_picker: Slot,
    set_color_picker_value: Slot,
    get_color_picker_value: Slot,
    get_builtin_page: Slot,
    input: [Slot; 7],
    storage: [Slot; 10],
    // API 1.2
    pub add_numeric_slider:
        Option<extern "C" fn(ProviderHandle, PageHandle, *const NumericSliderDesc, *mut RowHandle) -> Result>,
    pub set_numeric_slider_value: Option<extern "C" fn(ProviderHandle, RowHandle, *const DecimalValue) -> Result>,
    get_numeric_slider_value: Slot,
    pub add_custom_slider:
        Option<extern "C" fn(ProviderHandle, PageHandle, *const CustomSliderDesc, *mut RowHandle) -> Result>,
    pub set_custom_slider_index: Option<extern "C" fn(ProviderHandle, RowHandle, u32) -> Result>,
    get_custom_slider_index: Slot,
    // API 1.3
    pub add_tab: Option<extern "C" fn(ProviderHandle, *const TabDesc, *mut PageHandle) -> Result>,
    add_title_screen_text: Slot,
}

impl Api {
    pub fn zeroed() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

pub type GetApiFn = extern "C" fn(requested_version: u32, out_api: *mut Api) -> Result;

/// NativeTweaks fork: ERNativeUIFork_SetCustomSliderValues (any thread).
pub type SetCustomSliderValuesFn = extern "C" fn(ProviderHandle, RowHandle, *const Utf16View, u32) -> Result;

/// NativeTweaks fork: ERNativeUIFork_SetTextInputReadOnly (TextInput rows;
/// read-only rows never open the editor, values still repaint at once).
pub type SetTextInputReadOnlyFn = extern "C" fn(ProviderHandle, RowHandle, u32) -> Result;

/// NativeTweaks fork: ERNativeUIFork_SetRowText (any thread).
pub type SetRowTextFn =
    extern "C" fn(ProviderHandle, RowHandle, *const Utf16View, *const Utf16View) -> Result;

const _: () = {
    use std::mem::size_of;
    assert!(size_of::<StringView>() == 16);
    assert!(size_of::<Utf16View>() == 16);
    assert!(size_of::<ProviderDesc>() == 56);
    assert!(size_of::<ButtonDesc>() == 64);
    assert!(size_of::<ToggleDesc>() == 64);
    assert!(size_of::<ChoiceDesc>() == 72);
    assert!(size_of::<SubmenuDesc>() == 80);
    assert!(size_of::<TextInputChangeContext>() == 48);
    assert!(size_of::<TextInputDesc>() == 96);
    assert!(size_of::<DecimalValue>() == 16);
    assert!(size_of::<NumericSliderChangeContext>() == 56);
    assert!(size_of::<NumericSliderDesc>() == 136);
    assert!(size_of::<TabIconDesc>() == 32);
    assert!(size_of::<TabDesc>() == 64);
    assert!(size_of::<CustomSliderDesc>() == 80);
    assert!(size_of::<CustomSliderChangeContext>() == 40);
    assert!(size_of::<Api>() == API_V1_3_SIZE as usize);
};
