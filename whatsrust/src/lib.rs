use std::{
    collections::HashMap,
    ffi::{c_char, c_void},
    rc::Rc,
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

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct JID {
    pub user: Rc<str>,
    pub raw_agent: u8,
    pub device: u16,
    pub integrator: u16,
    pub server: Rc<str>,
}

impl From<&CJID> for JID {
    fn from(cjid: &CJID) -> Self {
        let user = unsafe { std::ffi::CStr::from_ptr(cjid.user) }
            .to_string_lossy()
            .into_owned()
            .into();
        let server = unsafe { std::ffi::CStr::from_ptr(cjid.server) }
            .to_string_lossy()
            .into_owned()
            .into();

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
        let user = std::ffi::CString::new(jid.user.as_ref()).unwrap();
        let server = std::ffi::CString::new(jid.server.as_ref()).unwrap();

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

#[repr(C)]
struct CMessageInfo {
    id: *const c_char,
    chat: CJID,
    sender: CJID,
    timestamp: i64,
    quote_id: *const c_char,
}

#[derive(Clone, Debug)]
pub struct MessageInfo {
    pub id: Rc<str>,
    pub chat: JID,
    pub sender: JID,
    pub timestamp: i64,
    pub quote_id: Option<Rc<str>>,
}

#[repr(C)]
struct CTextMessage {
    info: CMessageInfo,
    message: *const c_char,
}

#[derive(Clone, Debug)]
pub struct TextMessage(pub MessageInfo, pub Rc<str>);

// Rust-friendly version
#[derive(Clone, Debug)]
pub struct Contact {
    pub found: bool,
    pub first_name: Rc<str>,
    pub full_name: Rc<str>,
    pub push_name: Rc<str>,
    pub business_name: Rc<str>,
}

impl From<&CContact> for Contact {
    fn from(ccontact: &CContact) -> Self {
        let first_name = unsafe { std::ffi::CStr::from_ptr(ccontact.first_name) }
            .to_string_lossy()
            .into_owned()
            .into();
        let full_name = unsafe { std::ffi::CStr::from_ptr(ccontact.full_name) }
            .to_string_lossy()
            .into_owned()
            .into();
        let push_name = unsafe { std::ffi::CStr::from_ptr(ccontact.push_name) }
            .to_string_lossy()
            .into_owned()
            .into();
        let business_name = unsafe { std::ffi::CStr::from_ptr(ccontact.business_name) }
            .to_string_lossy()
            .into_owned()
            .into();

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
type CMessageCallback = extern "C" fn(*const CTextMessage, *mut c_void);
type CEventCallback = extern "C" fn(*mut c_void);
type CHistorySyncCallback = extern "C" fn(u32, *mut c_void);
unsafe extern "C" {
    fn C_NewClient(db_path: *const c_char);
    fn C_Connect(qr_cb: CQrCallback, data: *mut c_void) -> bool;
    fn C_SendMessage(jid: *const CJID, message: *const c_char);
    fn C_AddEventHandlers();
    fn C_GetAllContacts() -> CContactsMapResult;
    fn C_Disconnect();
    fn C_PairPhone(phone: *const c_char) -> *const c_char;

    fn C_SetMessageHandler(message_cb: CMessageCallback, data: *mut c_void);
    fn C_SetHistorySyncHandler(history_sync_cb: CHistorySyncCallback, data: *mut c_void);
    fn C_SetLogHandler(log_fn: LogFn);
    fn C_SetStateSyncCompleteHandler(event_cb: CEventCallback, data: *mut c_void);
}

pub fn pair_phone(phone: &str) -> String {
    let phone_c = std::ffi::CString::new(phone).unwrap();
    let result = unsafe { C_PairPhone(phone_c.as_ptr()) };
    let result_str = unsafe { std::ffi::CStr::from_ptr(result) }
        .to_string_lossy()
        .into_owned();
    result_str
}

pub fn add_event_handlers() {
    unsafe { C_AddEventHandlers() }
}

pub fn set_log_handler(log_fn: LogFn) {
    unsafe { C_SetLogHandler(log_fn) }
}

pub fn new_client(db_path: &str) {
    let db_path_c = std::ffi::CString::new(db_path).unwrap();
    unsafe { C_NewClient(db_path_c.as_ptr()) }
}

struct SetMessageHandler;
impl CallbackTranslator for SetMessageHandler {
    type CType = *const CTextMessage;
    type RustType = TextMessage;

    unsafe fn to_rust(ptr: Self::CType) -> Self::RustType {
        let message_info = unsafe { &(*ptr).info };
        let id = unsafe { std::ffi::CStr::from_ptr(message_info.id) }
            .to_string_lossy()
            .into_owned()
            .into();
        let chat: JID = (&message_info.chat).into();
        let sender: JID = (&message_info.sender).into();
        let message = unsafe { std::ffi::CStr::from_ptr((*ptr).message) }
            .to_string_lossy()
            .into_owned()
            .into();

        let c_quote_id = message_info.quote_id;
        let quote_id = if c_quote_id.is_null() {
            None
        } else {
            Some(
                unsafe { std::ffi::CStr::from_ptr(c_quote_id) }
                    .to_string_lossy()
                    .into_owned()
                    .into(),
            )
        };

        TextMessage(
            MessageInfo {
                id,
                chat,
                sender,
                timestamp: message_info.timestamp,
                quote_id,
            },
            message,
        )
    }

    unsafe fn invoke_closure(
        closure: &mut Box<dyn FnMut(Self::RustType)>,
        rust_value: Self::RustType,
    ) {
        closure(rust_value);
    }
}
define_callback!(
    set_message_handler_impl,
    C_SetMessageHandler,
    SetMessageHandler
);
pub fn set_message_handler<F>(handler: F)
where
    F: FnMut(TextMessage) + 'static,
{
    set_message_handler_impl(handler)
}

struct HistorySyncHandler;
impl CallbackTranslator for HistorySyncHandler {
    type CType = u32;
    type RustType = u32;

    unsafe fn to_rust(ptr: Self::CType) -> Self::RustType {
        ptr
    }

    unsafe fn invoke_closure(
        closure: &mut Box<dyn FnMut(Self::RustType)>,
        rust_value: Self::RustType,
    ) {
        closure(rust_value);
    }
}
define_callback!(
    history_sync_handler_impl,
    C_SetHistorySyncHandler,
    HistorySyncHandler
);
pub fn set_history_sync_handler<F>(handler: F)
where
    F: FnMut(u32) + 'static,
{
    history_sync_handler_impl(handler)
}

struct StateSyncCompleteHandler;
impl CallbackTranslator for StateSyncCompleteHandler {
    type CType = ();
    type RustType = ();

    unsafe fn to_rust(_ptr: Self::CType) -> Self::RustType {}

    unsafe fn invoke_closure(
        closure: &mut Box<dyn FnMut(Self::RustType)>,
        _rust_value: Self::RustType,
    ) {
        closure(())
    }
}
define_callback!(
    state_sync_complete_handler_impl,
    C_SetStateSyncCompleteHandler,
    StateSyncCompleteHandler where CType is ()
);
pub fn set_state_sync_complete_handler<F>(mut handler: F)
where
    F: FnMut() + 'static,
{
    state_sync_complete_handler_impl(move |_| handler())
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
    let result = unsafe { C_GetAllContacts() };

    let jids = unsafe { std::slice::from_raw_parts(result.jids, result.count as usize) };
    let contacts = unsafe { std::slice::from_raw_parts(result.contacts, result.count as usize) };

    let contacts_map: HashMap<JID, Contact> = jids
        .iter()
        .zip(contacts.iter())
        .map(|(jid, contact)| {
            let jid = JID::from(jid);
            let contact = Contact::from(contact);
            (jid, contact)
        })
        .collect();
    contacts_map
}
