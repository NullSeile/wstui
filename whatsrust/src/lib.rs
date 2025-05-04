use std::{
    collections::HashMap,
    ffi::{c_char, c_void},
    rc::Rc,
    sync::{Arc, Mutex},
};

#[macro_use]
mod callbacks;
use callbacks::CallbackTranslator;

type CJID = *const c_char;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct JID(Rc<str>);

impl From<JID> for Rc<str> {
    fn from(jid: JID) -> Self {
        jid.0
    }
}

impl From<&CJID> for JID {
    fn from(cjid: &CJID) -> Self {
        JID(unsafe { std::ffi::CStr::from_ptr(*cjid) }
            .to_string_lossy()
            .into())
    }
}
impl From<&JID> for CJID {
    fn from(jid: &JID) -> Self {
        std::ffi::CString::new(jid.0.as_ref()).unwrap().into_raw()
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
struct CContactsMapResult {
    jids: *const CJID,
    contacts: *const CContact,
    count: u32,
}

#[repr(C)]
struct CMessageInfo {
    id: *const c_char,
    chat: CJID,
    sender: CJID,
    timestamp: i64,
    quote_id: *const c_char,
}

#[repr(C)]
struct CGroupInfo {
    jid: CJID,
    name: *const c_char,
}

#[repr(C)]
struct CGetGroupInfoResult {
    groups: *const CGroupInfo,
    count: u32,
}

#[repr(C)]
struct CTextMessage {
    text: *const c_char,
}

#[repr(C)]
struct CImageMessage {
    path: *const c_char,
    file_id: *const c_char,
    caption: *const c_char,
}

#[repr(C)]
struct CMessage {
    info: CMessageInfo,
    message_type: i8,
    message: *const c_void,
}

pub type MessageId = Rc<str>;

#[derive(Clone, Debug)]
pub struct MessageInfo {
    pub id: MessageId,
    pub chat: JID,
    pub sender: JID,
    pub timestamp: i64,
    pub quote_id: Option<Rc<str>>,
}

enum MessageType {
    Text = 0,
    Image = 1,
}

fn get_message_type(message_type: i8) -> MessageType {
    match message_type {
        0 => MessageType::Text,
        1 => MessageType::Image,
        _ => panic!("Unknown message type"),
    }
}

pub type FileId = Rc<str>;

#[derive(Clone, Debug)]
pub struct ImageContent {
    pub path: Rc<str>,
    pub file_id: FileId,
    pub caption: Option<Rc<str>>,
}

#[derive(Clone, Debug)]
pub enum MessageContent {
    Text(Rc<str>),
    Image(ImageContent),
}

#[derive(Clone, Debug)]
pub struct Message {
    pub info: MessageInfo,
    pub message: MessageContent,
}

#[derive(Clone, Debug)]
pub struct Contact {
    pub found: bool,
    pub first_name: Rc<str>,
    pub full_name: Rc<str>,
    pub push_name: Rc<str>,
    pub business_name: Rc<str>,
}

#[derive(Clone, Debug)]
pub struct GroupInfo {
    pub jid: JID,
    pub name: Rc<str>,
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

pub type LogFn = extern "C" fn(*const c_char, u8);

type CQrCallback = extern "C" fn(*const c_char, *mut c_void);
type CMessageCallback = extern "C" fn(*const CMessage, *mut c_void);
type CEventCallback = extern "C" fn(*mut c_void);
type CHistorySyncCallback = extern "C" fn(u32, *mut c_void);
unsafe extern "C" {
    fn C_NewClient(db_path: *const c_char);
    fn C_Connect(qr_cb: CQrCallback, data: *mut c_void) -> bool;
    fn C_SendMessage(jid: CJID, message: *const c_char);
    fn C_AddEventHandlers();
    fn C_GetAllContacts() -> CContactsMapResult;
    fn C_GetJoinedGroups() -> CGetGroupInfoResult;
    fn C_Disconnect();
    fn C_PairPhone(phone: *const c_char) -> *const c_char;
    fn C_DownloadFile(file_id: *const c_char) -> u8;

    fn C_SetMessageHandler(message_cb: CMessageCallback, data: *mut c_void);
    fn C_SetHistorySyncHandler(history_sync_cb: CHistorySyncCallback, data: *mut c_void);
    fn C_SetLogHandler(log_fn: LogFn);
    fn C_SetStateSyncCompleteHandler(event_cb: CEventCallback, data: *mut c_void);
}

pub struct DownloadFailed;

pub fn download_file(file_id: &FileId) -> Result<(), DownloadFailed> {
    let file_id_c = std::ffi::CString::new(file_id.as_ref()).unwrap();
    let code = unsafe { C_DownloadFile(file_id_c.as_ptr()) };
    if code == 0 {
        Ok(())
    } else {
        Err(DownloadFailed)
    }
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

impl CallbackTranslator<*const CMessage> for Message {
    unsafe fn to_rust(ptr: *const CMessage) -> Self {
        let msg = unsafe { &(*ptr) };
        let id = unsafe { std::ffi::CStr::from_ptr(msg.info.id) }
            .to_string_lossy()
            .into_owned()
            .into();
        let chat: JID = (&msg.info.chat).into();
        let sender: JID = (&msg.info.sender).into();

        let c_quote_id = msg.info.quote_id;
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

        let message = match get_message_type(msg.message_type) {
            MessageType::Text => {
                let text_message = unsafe { &*(msg.message as *const CTextMessage) };

                let message = unsafe { std::ffi::CStr::from_ptr(text_message.text) }
                    .to_string_lossy()
                    .into_owned()
                    .into();
                MessageContent::Text(message)
            }
            MessageType::Image => {
                let image_message = unsafe { &*(msg.message as *const CImageMessage) };

                let path = unsafe { std::ffi::CStr::from_ptr(image_message.path) }
                    .to_string_lossy()
                    .into_owned()
                    .into();

                let file_id = unsafe { std::ffi::CStr::from_ptr(image_message.file_id) }
                    .to_string_lossy()
                    .into_owned()
                    .into();

                let caption = if image_message.caption.is_null() {
                    None
                } else {
                    Some(
                        unsafe { std::ffi::CStr::from_ptr(image_message.caption) }
                            .to_string_lossy()
                            .into_owned()
                            .into(),
                    )
                };
                MessageContent::Image(ImageContent {
                    path,
                    file_id,
                    caption,
                })
            }
        };

        Message {
            info: MessageInfo {
                id,
                chat,
                sender,
                timestamp: msg.info.timestamp,
                quote_id,
            },
            message,
        }
    }
}

setup_handler!(
    set_message_handler,
    C_SetMessageHandler,
    *const CMessage => Message
);

impl CallbackTranslator<u32> for u32 {
    unsafe fn to_rust(ptr: u32) -> u32 {
        ptr
    }
}
setup_handler!(set_history_sync_handler, C_SetHistorySyncHandler, u32 => u32);

setup_handler!(
    set_state_sync_complete_handler,
    C_SetStateSyncCompleteHandler,
);

impl CallbackTranslator<*const c_char> for String {
    unsafe fn to_rust(ptr: *const c_char) -> String {
        let c_str = unsafe { std::ffi::CStr::from_ptr(ptr) };
        c_str.to_string_lossy().into_owned()
    }
}
setup_handler!(connect, C_Connect, *const c_char => String);

pub fn disconnect() {
    unsafe { C_Disconnect() }
}

pub fn send_message(jid: &JID, message: &str) {
    let message_c = std::ffi::CString::new(message).unwrap();
    let jid_c = CJID::from(jid);
    unsafe { C_SendMessage(jid_c, message_c.as_ptr()) }
}

pub fn get_joined_groups() -> Vec<GroupInfo> {
    let result = unsafe { C_GetJoinedGroups() };
    let groups = unsafe { std::slice::from_raw_parts(result.groups, result.count as usize) };

    groups
        .iter()
        .map(|group| {
            let jid: JID = (&group.jid).into();
            let name = unsafe { std::ffi::CStr::from_ptr(group.name) }
                .to_string_lossy()
                .into_owned()
                .into();
            GroupInfo { jid, name }
        })
        .collect()
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
