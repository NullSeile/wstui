use std::{
    collections::HashMap,
    ffi::{c_char, c_void},
    sync::{Arc, Mutex},
};

#[macro_use]
mod callbacks;
use callbacks::CallbackTranslator;

#[repr(C)]
struct CJID {
    user: *mut c_char,
    raw_agent: u8,
    device: u16,
    integrator: u16,
    server: *mut c_char,
}

// Rust-friendly version
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct JID {
    pub user: String,
    pub raw_agent: u8,
    pub device: u16,
    pub integrator: u16,
    pub server: String,
}

impl From<&CJID> for JID {
    fn from(cjid: &CJID) -> Self {
        let user = unsafe { std::ffi::CStr::from_ptr(cjid.user) }
            .to_string_lossy()
            .into_owned();
        let server = unsafe { std::ffi::CStr::from_ptr(cjid.server) }
            .to_string_lossy()
            .into_owned();

        JID {
            user,
            raw_agent: cjid.raw_agent,
            device: cjid.device,
            integrator: cjid.integrator,
            server,
        }
    }
}

impl From<&JID> for CJID {
    fn from(jid: &JID) -> Self {
        let user = std::ffi::CString::new(jid.user.clone()).unwrap();
        let server = std::ffi::CString::new(jid.server.clone()).unwrap();

        CJID {
            user: user.into_raw(),
            raw_agent: jid.raw_agent,
            device: jid.device,
            integrator: jid.integrator,
            server: server.into_raw(),
        }
    }
}

#[repr(C)]
struct CContact {
    found: bool,
    first_name: *const c_char,
    full_name: *const c_char,
    push_name: *const c_char,
    business_name: *const c_char,
}

// Rust-friendly version
#[derive(Clone, Debug)]
pub struct Contact {
    pub found: bool,
    pub first_name: String,
    pub full_name: String,
    pub push_name: String,
    pub business_name: String,
}

impl From<&CContact> for Contact {
    fn from(ccontact: &CContact) -> Self {
        let first_name = unsafe { std::ffi::CStr::from_ptr(ccontact.first_name) }
            .to_string_lossy()
            .into_owned();
        let full_name = unsafe { std::ffi::CStr::from_ptr(ccontact.full_name) }
            .to_string_lossy()
            .into_owned();
        let push_name = unsafe { std::ffi::CStr::from_ptr(ccontact.push_name) }
            .to_string_lossy()
            .into_owned();
        let business_name = unsafe { std::ffi::CStr::from_ptr(ccontact.business_name) }
            .to_string_lossy()
            .into_owned();

        Contact {
            found: ccontact.found,
            first_name,
            full_name,
            push_name,
            business_name,
        }
    }
}

#[repr(C)]
struct CContactsMapResult {
    jids: *const CJID,
    contacts: *const CContact,
    count: u32,
}

pub type LogFn = extern "C" fn(*const c_char, u8);

type CQrCallback = extern "C" fn(*const c_char, *mut c_void);
type CEventHandler = extern "C" fn(*const c_char, *mut c_void);
unsafe extern "C" {
    fn C_NewClient(db_path: *const c_char);
    fn C_Connect(qr_cb: CQrCallback, data: *mut c_void) -> bool;
    fn C_SendMessage(jid: *const CJID, message: *const c_char);
    fn C_AddEventHandler(handler: CEventHandler, data: *mut c_void);
    fn C_GetAllContacts() -> CContactsMapResult;
    fn C_Disconnect();
    pub fn C_SetLogHandler(log_fn: LogFn);
    // fn (log_fn: extern "C" fn(*const c_char, *mut c_void), data: *mut c_void);
}

pub fn new_client(db_path: &str) {
    let db_path_c = std::ffi::CString::new(db_path).unwrap();
    unsafe { C_NewClient(db_path_c.as_ptr()) }
}

struct EventHandler;
impl CallbackTranslator for EventHandler {
    type CType = *const c_char;
    type RustType = String;

    unsafe fn to_rust(ptr: Self::CType) -> Self::RustType {
        let c_str = unsafe { std::ffi::CStr::from_ptr(ptr) };
        c_str.to_string_lossy().into_owned()
    }

    unsafe fn invoke_closure(
        closure: &mut Box<dyn FnMut(Self::RustType)>,
        rust_value: Self::RustType,
    ) {
        closure(rust_value);
    }
}

define_callback!(add_event_handler_impl, C_AddEventHandler, EventHandler);
pub fn add_event_handler<F>(handler: F)
where
    F: FnMut(String) + 'static,
{
    add_event_handler_impl(handler)
}

struct QrCallback;
impl CallbackTranslator for QrCallback {
    type CType = *const c_char;
    type RustType = String;

    unsafe fn to_rust(ptr: Self::CType) -> Self::RustType {
        let c_str = unsafe { std::ffi::CStr::from_ptr(ptr) };
        c_str.to_string_lossy().into_owned()
    }

    unsafe fn invoke_closure(
        closure: &mut Box<dyn FnMut(Self::RustType)>,
        rust_value: Self::RustType,
    ) {
        closure(rust_value);
    }
}

define_callback!(connect_impl, C_Connect, QrCallback);
pub fn connect<F>(handler: F)
where
    F: FnMut(String) + 'static,
{
    connect_impl(handler)
}

pub fn disconnect() {
    unsafe { C_Disconnect() }
}

pub fn send_message(jid: &JID, message: &str) {
    let message_c = std::ffi::CString::new(message).unwrap();
    let jid_c = CJID::from(jid);
    unsafe { C_SendMessage(&jid_c, message_c.as_ptr()) }
}

pub fn get_all_contacts() -> HashMap<JID, Contact> {
    let mut contacts_map: HashMap<JID, Contact> = HashMap::new();

    let result = unsafe { C_GetAllContacts() };

    let jids = unsafe { std::slice::from_raw_parts(result.jids, result.count as usize) };
    let contacts = unsafe { std::slice::from_raw_parts(result.contacts, result.count as usize) };

    for (jid, contact) in jids.iter().zip(contacts.iter()) {
        let jid: JID = jid.into();
        let contact: Contact = contact.into();
        contacts_map.insert(jid, contact);
    }

    contacts_map
}
