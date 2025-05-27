use std::{
    collections::HashMap,
    ffi::{CStr, CString, c_char, c_void},
    path::Path,
    sync::{Arc, Mutex},
};

#[macro_use]
mod callbacks;
use callbacks::CallbackTranslator;
use log::info;
use strum::{EnumIter, FromRepr};

type CJID = *const c_char;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct JID(pub Arc<str>);

impl From<JID> for Arc<str> {
    fn from(jid: JID) -> Self {
        jid.0
    }
}
impl From<String> for JID {
    fn from(jid: String) -> Self {
        JID(jid.into())
    }
}

impl From<&CJID> for JID {
    fn from(cjid: &CJID) -> Self {
        JID(unsafe { CStr::from_ptr(*cjid) }.to_string_lossy().into())
    }
}
impl From<&JID> for CJID {
    fn from(jid: &JID) -> Self {
        CString::new(jid.0.as_ref()).unwrap().into_raw()
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
    is_from_me: bool,
    quote_id: *const c_char,
    is_read: bool,
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
struct CFileMessage {
    kind: u8,
    path: *const c_char,
    file_id: *const c_char,
    caption: *const c_char,
}

#[repr(C)]
struct CMessage {
    info: CMessageInfo,
    message_type: u8,
    message: *const c_void,
}

#[derive(Clone, Debug)]
#[repr(C)]
struct CEvent {
    event_type: u8,
    data: *const c_void,
}

pub type MessageId = Arc<str>;

#[derive(Clone, Debug)]
pub struct MessageInfo {
    pub id: MessageId,
    pub chat: JID,
    pub sender: JID,
    pub timestamp: i64,
    pub is_from_me: bool,
    pub quote_id: Option<Arc<str>>,
    pub is_read: bool,
}

#[derive(FromRepr)]
#[repr(u8)]
enum MessageType {
    Text = 0,
    File = 1,
}

#[derive(Clone, Debug, Default, FromRepr)]
#[repr(u8)]
pub enum FileKind {
    #[default]
    Image = 0,
    Video = 1,
    Audio = 2,
    Document = 3,
    Sticker = 4,
}

#[derive(Clone, Debug, FromRepr)]
#[repr(u8)]
enum EventType {
    SyncProgress = 0,
    AppStateSyncComplete = 1,
}

#[derive(Clone, Debug)]
pub enum Event {
    SyncProgress(u8),
    AppStateSyncComplete,
}

pub type FileId = Arc<str>;

#[derive(Clone, Debug, Default)]
pub struct FileContent {
    pub kind: FileKind,
    pub path: Arc<str>,
    pub file_id: FileId,
    pub caption: Option<Arc<str>>,
}

#[derive(Clone, Debug, EnumIter)]
pub enum MessageContent {
    Text(Arc<str>),
    File(FileContent),
}

#[derive(Clone, Debug)]
pub struct Message {
    pub info: MessageInfo,
    pub message: MessageContent,
}

#[derive(Clone, Debug)]
pub struct Contact {
    pub found: bool,
    pub first_name: Arc<str>,
    pub full_name: Arc<str>,
    pub push_name: Arc<str>,
    pub business_name: Arc<str>,
}

#[derive(Clone, Debug)]
pub struct GroupInfo {
    pub jid: JID,
    pub name: Arc<str>,
}

impl From<&CContact> for Contact {
    fn from(ccontact: &CContact) -> Self {
        let first_name = unsafe { CStr::from_ptr(ccontact.first_name) }
            .to_string_lossy()
            .into_owned()
            .into();
        let full_name = unsafe { CStr::from_ptr(ccontact.full_name) }
            .to_string_lossy()
            .into_owned()
            .into();
        let push_name = unsafe { CStr::from_ptr(ccontact.push_name) }
            .to_string_lossy()
            .into_owned()
            .into();
        let business_name = unsafe { CStr::from_ptr(ccontact.business_name) }
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

type CLogCallback = extern "C" fn(*const c_char, u8, *mut c_void);
type CQrCallback = extern "C" fn(*const c_char, *mut c_void);
type CMessageCallback = extern "C" fn(*const CMessage, bool, *mut c_void);
type CEventCallback = extern "C" fn(*const CEvent, *mut c_void);
unsafe extern "C" {
    fn C_NewClient(db_path: *const c_char);
    fn C_Connect(qr_cb: CQrCallback, data: *mut c_void);
    fn C_SendMessage(jid: CJID, message: *const c_char, quoted_message: *const CMessageInfo);
    fn C_GetAllContacts() -> CContactsMapResult;
    fn C_GetJoinedGroups() -> CGetGroupInfoResult;
    fn C_Disconnect();
    fn C_PairPhone(phone: *const c_char) -> *const c_char;
    fn C_DownloadFile(file_id: *const c_char, base_path: *const c_char) -> u8;

    fn C_SetMessageHandler(message_cb: CMessageCallback, data: *mut c_void);
    fn C_SetEventHandler(event_cb: CEventCallback, data: *mut c_void);
    fn C_SetLogHandler(log_fn: CLogCallback, data: *mut c_void);
}

pub struct DownloadFailed;

pub fn download_file(file_id: &FileId, base_path: &Path) -> Result<(), DownloadFailed> {
    let file_id_c = CString::new(file_id.as_ref()).unwrap();
    let base_path_c = CString::new(base_path.to_str().unwrap()).unwrap();
    let code = unsafe { C_DownloadFile(file_id_c.as_ptr(), base_path_c.as_ptr()) };
    if code == 0 {
        Ok(())
    } else {
        Err(DownloadFailed)
    }
}

pub fn pair_phone(phone: &str) -> String {
    let phone_c = CString::new(phone).unwrap();
    let result = unsafe { C_PairPhone(phone_c.as_ptr()) };
    let result_str = unsafe { CStr::from_ptr(result) }
        .to_string_lossy()
        .into_owned();
    result_str
}

pub fn new_client(db_path: &str) {
    let db_path_c = CString::new(db_path).unwrap();
    unsafe { C_NewClient(db_path_c.as_ptr()) }
}

impl CallbackTranslator<*const CEvent> for Event {
    unsafe fn to_rust(ptr: *const CEvent) -> Self {
        let event = unsafe { &(*ptr) };
        match EventType::from_repr(event.event_type).unwrap() {
            EventType::SyncProgress => {
                let percent = unsafe { *(event.data as *const u8) };
                Event::SyncProgress(percent)
            }
            EventType::AppStateSyncComplete => Event::AppStateSyncComplete,
        }
    }
}

setup_handler!(
    set_event_handler,
    C_SetEventHandler,
    event: *const CEvent => Event
);

impl CallbackTranslator<*const CMessage> for Message {
    unsafe fn to_rust(ptr: *const CMessage) -> Self {
        let msg = unsafe { &(*ptr) };
        let id = unsafe { CStr::from_ptr(msg.info.id) }
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
                unsafe { CStr::from_ptr(c_quote_id) }
                    .to_string_lossy()
                    .into_owned()
                    .into(),
            )
        };

        let message = match MessageType::from_repr(msg.message_type).unwrap() {
            MessageType::Text => {
                let text_message = unsafe { &*(msg.message as *const CTextMessage) };

                let message = unsafe { CStr::from_ptr(text_message.text) }
                    .to_string_lossy()
                    .into_owned()
                    .into();
                MessageContent::Text(message)
            }
            MessageType::File => {
                let image_message = unsafe { &*(msg.message as *const CFileMessage) };

                let path = unsafe { CStr::from_ptr(image_message.path) }
                    .to_string_lossy()
                    .into_owned()
                    .into();

                let file_id = unsafe { CStr::from_ptr(image_message.file_id) }
                    .to_string_lossy()
                    .into_owned()
                    .into();

                let caption = if image_message.caption.is_null() {
                    None
                } else {
                    Some(
                        unsafe { CStr::from_ptr(image_message.caption) }
                            .to_string_lossy()
                            .into_owned()
                            .into(),
                    )
                };
                MessageContent::File(FileContent {
                    kind: FileKind::from_repr(image_message.kind).unwrap(),
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
                is_from_me: msg.info.is_from_me,
                quote_id,
                is_read: msg.info.is_read,
            },
            message,
        }
    }
}

impl CallbackTranslator<bool> for bool {
    unsafe fn to_rust(ptr: bool) -> bool {
        ptr
    }
}

setup_handler!(
    set_message_handler,
    C_SetMessageHandler,
    msg: *const CMessage => Message,
    is_sync: bool => bool
);

impl CallbackTranslator<*const c_char> for String {
    unsafe fn to_rust(ptr: *const c_char) -> String {
        let c_str = unsafe { CStr::from_ptr(ptr) };
        c_str.to_string_lossy().into_owned()
    }
}

impl CallbackTranslator<u8> for u8 {
    unsafe fn to_rust(ptr: u8) -> u8 {
        ptr
    }
}

setup_handler!(
    set_log_handler,
    C_SetLogHandler,
    msg: *const c_char => String,
    level: u8 => u8
);

setup_handler!(connect, C_Connect, qr: *const c_char => String);

pub fn disconnect() {
    unsafe { C_Disconnect() }
}

pub fn send_message(jid: &JID, message: &str, quoted_message: Option<&Message>) {
    let message_c = CString::new(message).unwrap();
    let jid_c = CJID::from(jid);

    if let Some(quoted_message) = quoted_message {
        let quoted_chat = CJID::from(&quoted_message.info.chat);
        let quoted_sender = CJID::from(&quoted_message.info.sender);
        let quoted_id = CString::new(quoted_message.info.id.as_ref()).unwrap();

        let info = CMessageInfo {
            id: quoted_id.as_ptr(),
            chat: quoted_chat,
            sender: quoted_sender,
            timestamp: quoted_message.info.timestamp,
            is_from_me: quoted_message.info.is_from_me,
            quote_id: quoted_message
                .info
                .quote_id
                .as_ref()
                .map_or(std::ptr::null(), |q| {
                    CString::new(q.as_ref()).unwrap().into_raw()
                }),
            is_read: quoted_message.info.is_read,
        };

        unsafe { C_SendMessage(jid_c, message_c.as_ptr(), &info as *const _) }
    } else {
        unsafe { C_SendMessage(jid_c, message_c.as_ptr(), std::ptr::null()) }
    }
}

pub fn get_joined_groups() -> Vec<GroupInfo> {
    let result = unsafe { C_GetJoinedGroups() };
    let groups = unsafe { std::slice::from_raw_parts(result.groups, result.count as usize) };

    groups
        .iter()
        .map(|group| {
            let jid: JID = (&group.jid).into();
            let name = unsafe { CStr::from_ptr(group.name) }
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
