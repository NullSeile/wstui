package main

/*
#include <stdlib.h>
#include <stdint.h>
#include <stdbool.h>
#include <stdio.h>

typedef const char* JID;

typedef struct {
	bool found;
	const char* first_name;
	const char* full_name;
	const char* push_name;
	const char* business_name;
} Contact;

typedef struct {
	JID* jids;
	Contact* contacts;
	uint32_t size;
} ContactsMapResult;

typedef struct {
	JID jid;
	const char* name;
} GroupInfo;

typedef struct {
	GroupInfo* groups;
	uint32_t size;
} GetJoinedGroupsResult;

typedef struct {
	char* id;
	JID chat;
	JID sender;
	int64_t timestamp;
	char* quoteID;
	bool isRead;
} MessageInfo;

typedef struct {
	char* text;
} TextMessage;

typedef struct {
	char* path;
	char* fileID;
	char* caption;
} ImageMessage;

typedef struct {
	MessageInfo info;
	int8_t messageType;
	void* message;
} Message;

typedef void (*QrCallback)(const char*, void*);
static void callQrCallback(QrCallback cb, const char* code, void* user_data) {
	cb(code, user_data);
}

typedef void (*MessageHandlerCallback)(const Message*, void*);
typedef struct {
	MessageHandlerCallback callback;
	void* user_data;
} MessageHandler;
static void callMessageHandler(MessageHandler hdl, const Message* data) {
    hdl.callback(data, hdl.user_data);
}

typedef void (*StateSyncCompleteCallback)(void*);
typedef struct {
	StateSyncCompleteCallback callback;
	void* user_data;
} StateSyncCompleteHandler;
static void callStateSyncComplete(StateSyncCompleteHandler hdl) {
	hdl.callback(hdl.user_data);
}

typedef void (*HistorySyncCallback)(uint32_t, void*);
typedef struct {
	HistorySyncCallback callback;
	void* user_data;
} HistorySyncHandler;
static void callHistorySync(HistorySyncHandler hdl, uint32_t percent) {
	hdl.callback(percent, hdl.user_data);
}

typedef void (*LogHandler)(const char*, uint8_t);
static void callLogInfo(LogHandler cb, const char* msg, uint8_t level) {
	cb(msg, level);
}
*/
import "C"
import (
	"context"
	"fmt"
	"math"
	"mime"
	"slices"
	"sort"
	"time"
	"unsafe"

	_ "github.com/mattn/go-sqlite3"
	"go.mau.fi/whatsmeow"
	"go.mau.fi/whatsmeow/appstate"
	"go.mau.fi/whatsmeow/proto/waE2E"
	"go.mau.fi/whatsmeow/proto/waWeb"
	"go.mau.fi/whatsmeow/store/sqlstore"
	"go.mau.fi/whatsmeow/types"
	"go.mau.fi/whatsmeow/types/events"
	waLog "go.mau.fi/whatsmeow/util/log"
)

var client *whatsmeow.Client
var qrChan <-chan whatsmeow.QRChannelItem

var logHandler C.LogHandler
var messageHandler C.MessageHandler
var StateSyncCompleteHandler C.StateSyncCompleteHandler
var historySyncHandler C.HistorySyncHandler

func LOG_LEVEL(level int, msg string, args ...any) {
	if logHandler != nil {
		cmsg := C.CString(fmt.Sprintf(msg, args...))
		defer C.free(unsafe.Pointer(cmsg))
		C.callLogInfo(logHandler, cmsg, C.uint8_t(level))
	}
}

func LOG_ERROR(msg string, args ...any) {
	LOG_LEVEL(0, msg, args...)
}
func LOG_WARN(msg string, args ...any) {
	LOG_LEVEL(1, msg, args...)
}
func LOG_INFO(msg string, args ...any) {
	LOG_LEVEL(2, msg, args...)
}
func LOG_DEBUG(msg string, args ...any) {
	LOG_LEVEL(3, msg, args...)
}

// Logger
type WrLogger struct{}

func (l *WrLogger) Errorf(msg string, args ...any) {
	LOG_ERROR(msg, args...)
}
func (l *WrLogger) Warnf(msg string, args ...any) {
	LOG_WARN(msg, args...)
}
func (l *WrLogger) Infof(msg string, args ...any) {
	LOG_INFO(msg, args...)
}
func (l *WrLogger) Debugf(msg string, args ...any) {
	// LOG_DEBUG(msg, args...)
}

func (l *WrLogger) Sub(module string) waLog.Logger {
	return &WrLogger{}
}

// Convert Go JID to C JID
func jidToC(jid types.JID) C.JID {
	return C.CString(jid.User + "@" + jid.Server)
}

// Convert C JID to Go JID
func cToJid(cjid C.JID) types.JID {
	jid, err := types.ParseJID(C.GoString(cjid))
	if err != nil {
		panic(err)
	}
	return jid
}

// Convert Go ContactInfo to C Contact
func contactToC(contact types.ContactInfo) C.Contact {
	return C.Contact{
		found:         C.bool(contact.Found),
		first_name:    C.CString(contact.FirstName),
		full_name:     C.CString(contact.FullName),
		push_name:     C.CString(contact.PushName),
		business_name: C.CString(contact.BusinessName),
	}
}

//export C_SetLogHandler
func C_SetLogHandler(handler C.LogHandler) {
	logHandler = handler
}

//export C_SetHistorySyncHandler
func C_SetHistorySyncHandler(callback C.HistorySyncCallback, data unsafe.Pointer) {
	historySyncHandler = C.HistorySyncHandler{
		callback:  callback,
		user_data: data,
	}
}

//export C_SetMessageHandler
func C_SetMessageHandler(callback C.MessageHandlerCallback, data unsafe.Pointer) {
	messageHandler = C.MessageHandler{
		callback:  callback,
		user_data: data,
	}
}

//export C_SetStateSyncCompleteHandler
func C_SetStateSyncCompleteHandler(callback C.StateSyncCompleteCallback, data unsafe.Pointer) {
	StateSyncCompleteHandler = C.StateSyncCompleteHandler{
		callback:  callback,
		user_data: data,
	}
}

//export C_NewClient
func C_NewClient(dbPath *C.char) {
	goPath := C.GoString(dbPath)
	dbLog := &WrLogger{}
	container, err := sqlstore.New("sqlite3", "file:"+goPath+"?_foreign_keys=on", dbLog)
	if err != nil {
		panic(err)
	}
	deviceStore, _ := container.GetFirstDevice()
	clientLog := &WrLogger{}
	client = whatsmeow.NewClient(deviceStore, clientLog)
}

func ParseWebMessageInfo(selfJid types.JID, chatJid types.JID, webMsg *waWeb.WebMessageInfo) *types.MessageInfo {
	info := types.MessageInfo{
		MessageSource: types.MessageSource{
			Chat:     chatJid,
			IsFromMe: webMsg.GetKey().GetFromMe(),
			IsGroup:  chatJid.Server == types.GroupServer,
		},
		ID:        webMsg.GetKey().GetID(),
		PushName:  webMsg.GetPushName(),
		Timestamp: time.Unix(int64(webMsg.GetMessageTimestamp()), 0),
	}
	if info.IsFromMe {
		info.Sender = selfJid.ToNonAD()
	} else if webMsg.GetParticipant() != "" {
		info.Sender, _ = types.ParseJID(webMsg.GetParticipant())
	} else if webMsg.GetKey().GetParticipant() != "" {
		info.Sender, _ = types.ParseJID(webMsg.GetKey().GetParticipant())
	} else {
		info.Sender = chatJid
	}
	if info.Sender.IsEmpty() {
		return nil
	}
	return &info
}

func SliceIndex(list []string, value string, defaultValue int) int {
	index := slices.Index(list, value)
	if index == -1 {
		index = defaultValue
	}
	return index
}

func ExtensionByType(mimeType string, defaultExt string) string {
	ext := defaultExt
	exts, extErr := mime.ExtensionsByType(mimeType)
	if extErr == nil && len(exts) > 0 {
		// prefer common extensions over less common (.jpe, etc) returned by mime library
		preferredExts := []string{".jpg", ".jpeg"}
		sort.Slice(exts, func(i, j int) bool {
			return SliceIndex(preferredExts, exts[i], math.MaxInt32) < SliceIndex(preferredExts, exts[j], math.MaxInt32)
		})
		ext = exts[0]
	}

	return ext
}

const (
	MessageTypeText = iota
	MessageTypeImage
)

// C.Message to waE2E.Message
func CMessageToWaE2EMessage(cmsg *C.Message) (types.MessageInfo, *waE2E.Message) {
	info := types.MessageInfo{
		MessageSource: types.MessageSource{
			Chat:   cToJid(cmsg.info.chat),
			Sender: cToJid(cmsg.info.sender),
		},
		ID:        C.GoString(cmsg.info.id),
		Timestamp: time.Unix(int64(cmsg.info.timestamp), 0),
	}

	switch cmsg.messageType {
	case C.int8_t(MessageTypeText):
		textMsg := (*C.TextMessage)(cmsg.message)
		text := C.GoString(textMsg.text)
		LOG_INFO("Text: %v %s", textMsg.text, text)
		msg := waE2E.Message{
			Conversation: &text,
		}
		return info, &msg
	case C.int8_t(MessageTypeImage):
		imgMsg := (*C.ImageMessage)(cmsg.message)
		// downloadInfo, err := FileIdToDownloadInfo(C.GoString(imgMsg.fileID))
		// if err != nil {
		// 	panic(err)
		// }

		caption := C.GoString(imgMsg.caption)
		msg := waE2E.Message{
			ImageMessage: &waE2E.ImageMessage{
				Caption: &caption,
			},
		}
		return info, &msg
	default:
		return info, nil
	}
}

func HandleMessage(info types.MessageInfo, msg *waE2E.Message) {
	chat := info.Chat
	sender := info.Sender
	timestamp := info.Timestamp.Unix()

	cinfo := C.MessageInfo{
		id:        C.CString(info.ID),
		chat:      jidToC(chat),
		sender:    jidToC(sender),
		timestamp: C.int64_t(timestamp),
		quoteID:   nil,
		isRead:    C.bool(false),
		// isRead:    C.bool(info.IsFromMe),
	}

	if msg.Conversation != nil {
		ctext := C.CString(msg.GetConversation())
		defer C.free(unsafe.Pointer(ctext))

		content := (*C.TextMessage)(C.malloc(C.sizeof_TextMessage))
		content.text = ctext
		defer C.free(unsafe.Pointer(content))

		message := C.Message{
			info:        cinfo,
			messageType: C.int8_t(MessageTypeText),
			message:     unsafe.Pointer(content),
		}

		C.callMessageHandler(messageHandler, &message)
	}
	if msg.ExtendedTextMessage != nil {
		ext_msg := msg.GetExtendedTextMessage()

		text := ext_msg.GetText()
		ctext := C.CString(text)
		defer C.free(unsafe.Pointer(ctext))

		context_info := ext_msg.GetContextInfo()
		if context_info != nil {
			id := context_info.GetStanzaID()
			// LOG_ERROR("asdfasdf %s", co)
			if id != "" {
				cinfo.quoteID = C.CString(id)
			}
		}

		content := (*C.TextMessage)(C.malloc(C.sizeof_TextMessage))
		content.text = ctext
		defer C.free(unsafe.Pointer(content))

		message := C.Message{
			info:        cinfo,
			messageType: C.int8_t(MessageTypeText),
			message:     unsafe.Pointer(content),
		}
		C.callMessageHandler(messageHandler, &message)
	}

	if msg.ImageMessage != nil {
		img := msg.GetImageMessage()
		if img == nil {
			LOG_ERROR("ImageMessage is nil")
			return
		}

		ext := ExtensionByType(img.GetMimetype(), ".jpg")
		caption := img.GetCaption()

		context_info := img.GetContextInfo()
		if context_info != nil {
			id := context_info.GetStanzaID()
			if id != "" {
				cinfo.quoteID = C.CString(id)
			}
		}

		filePath := fmt.Sprintf("imgs/%s%s", info.ID, ext)

		fileId := DownloadableMessageToFileId(client, img, filePath)
		cfileId := C.CString(fileId)
		defer C.free(unsafe.Pointer(cfileId))

		cpath := C.CString(filePath)
		defer C.free(unsafe.Pointer(cpath))

		// set caption or nil
		ccaption := C.CString(caption)
		if caption == "" {
			ccaption = nil
		}
		defer C.free(unsafe.Pointer(ccaption))

		content := (*C.ImageMessage)(C.malloc(C.sizeof_ImageMessage))
		content.path = cpath
		content.fileID = cfileId
		content.caption = ccaption
		defer C.free(unsafe.Pointer(content))

		message := C.Message{
			info:        cinfo,
			messageType: C.int8_t(MessageTypeImage),
			message:     unsafe.Pointer(content),
		}
		C.callMessageHandler(messageHandler, &message)
	}
}

//export C_DownloadFile
func C_DownloadFile(fileId *C.char) C.uint8_t {
	goFileId := C.GoString(fileId)
	_, status := DownloadFromFileId(client, goFileId)
	return C.uint8_t(status)
}

//export C_AddEventHandlers
func C_AddEventHandlers() {
	client.AddEventHandler(func(rawEvt any) {
		switch evt := rawEvt.(type) {
		case *events.AppStateSyncComplete:
			LOG_ERROR("AppStateStateSyncComplete %v", evt)
			if evt.Name == appstate.WAPatchRegular {
				LOG_ERROR("AppStateStateSyncComplete %v", evt)
				if StateSyncCompleteHandler.callback != nil {
					C.callStateSyncComplete(StateSyncCompleteHandler)
				}
			}

		case *events.Message:
			HandleMessage(evt.Info, evt.Message)

		case *events.Receipt:
			if evt.Type == types.ReceiptTypeRead || evt.Type == types.ReceiptTypeReadSelf {
				LOG_INFO(fmt.Sprintf("%#v was read by %s at %s", evt.MessageIDs, evt.SourceString(), evt.Timestamp))
				// chatId := evt.MessageSource.Chat.ToNonAD().String()
				// isRead := true
				// for _, msgId := range evt.MessageIDs {
				// LOG_TRACE(fmt.Sprintf("Call CWmNewMessageStatusNotify"))
				// CWmNewMessageStatusNotify(connId, chatId, msgId, BoolToInt(isRead))
				// }
			}

		case *events.HistorySync:
			selfJid := *client.Store.ID

			percent := evt.Data.GetProgress()
			// LOG_WARN("History sync progress: %d %%", percent)
			if historySyncHandler.callback != nil {
				C.callHistorySync(historySyncHandler, C.uint32_t(percent))
			}

			conversations := evt.Data.GetConversations()
			for _, conversation := range conversations {
				chatJid, _ := types.ParseJID(conversation.GetID())
				syncMessages := conversation.GetMessages()

				for _, syncMessage := range syncMessages {
					webMessageInfo := syncMessage.Message
					messageInfo := ParseWebMessageInfo(selfJid, chatJid, webMessageInfo)
					message := webMessageInfo.GetMessage()

					if (messageInfo == nil) || (message == nil) {
						continue
					}

					HandleMessage(*messageInfo, message)
				}
			}
		}
	})
}

//export C_Connect
func C_Connect(handler C.QrCallback, data unsafe.Pointer) bool {
	if client.Store.ID == nil {
		qrChan, _ = client.GetQRChannel(context.Background())
		err := client.Connect()
		if err != nil {
			panic(err)
		}

		for evt := range qrChan {
			if evt.Event == "code" {
				code := C.CString(evt.Code)
				defer C.free(unsafe.Pointer(code))
				C.callQrCallback(handler, code, data)
			}
		}
		return false
	} else {
		err := client.Connect()
		if err != nil {
			panic(err)
		}
		return true
	}
}

//export C_PairPhone
func C_PairPhone(phone *C.char) *C.char {
	goPhone := C.GoString(phone)
	code, err := client.PairPhone(goPhone, true, whatsmeow.PairClientChrome, "Chrome (Linux)")
	if err != nil {
		panic(err)
	}
	cCode := C.CString(code)
	return cCode
}

// func C_SendMessage(cjid C.JID, ctext *C.char, cquoteId *C.char, cquotedSender C.JID) {
//
//export C_SendMessage
func C_SendMessage(cjid C.JID, ctext *C.char, quoted_msg *C.Message) {
	jid := cToJid(cjid)
	text := C.GoString(ctext)

	// LOG_INFO("%v", cquoteId)

	contextInfo := &waE2E.ContextInfo{}
	if quoted_msg != nil {
		info, msg := CMessageToWaE2EMessage(quoted_msg)
		contextInfo.StanzaID = &info.ID
		contextInfo.QuotedMessage = msg
		quotedSender := info.Sender.String()
		contextInfo.Participant = &quotedSender

		LOG_INFO("ID %#v", info.ID)
		LOG_INFO("Sender %#v", info.Sender.String())

		// LOG_INFO("Info %#v", info)
		// LOG_INFO("msg %#v", msg)
		// LOG_INFO("QuotedMessage %#v", contextInfo)
	}

	var message waE2E.Message
	message.ExtendedTextMessage = &waE2E.ExtendedTextMessage{
		Text:        &text,
		ContextInfo: contextInfo,
	}
	sendResponse, err := client.SendMessage(context.Background(), jid, &message)
	if err != nil {
		panic(err)
	} else {
		var messageInfo types.MessageInfo
		messageInfo.Chat = jid
		messageInfo.IsFromMe = true
		messageInfo.Sender = *client.Store.ID

		messageInfo.ID = sendResponse.ID
		messageInfo.Timestamp = sendResponse.Timestamp

		HandleMessage(messageInfo, &message)
	}
}

// TODO: Free the memory allocated for C.JID and C.Contact

//export C_GetJoinedGroups
func C_GetJoinedGroups() C.GetJoinedGroupsResult {
	groups, err := client.GetJoinedGroups()
	if err != nil {
		panic(err)
	}

	n := len(groups)
	c_groups := C.malloc(C.size_t(n) * C.size_t(unsafe.Sizeof(C.GroupInfo{})))
	groupList := unsafe.Slice((*C.GroupInfo)(c_groups), n)

	i := 0
	for _, group := range groups {
		groupList[i] = C.GroupInfo{
			jid:  jidToC(group.JID),
			name: C.CString(group.GroupName.Name),
		}
		i++
	}

	result := C.GetJoinedGroupsResult{
		groups: (*C.GroupInfo)(c_groups),
		size:   C.uint32_t(n),
	}
	return result
}

//export C_GetAllContacts
func C_GetAllContacts() C.ContactsMapResult {
	contacts, err := client.Store.Contacts.GetAllContacts()
	if err != nil {
		panic(err)
	}

	n := len(contacts)
	c_jids := C.malloc(C.size_t(n) * C.size_t(C.sizeof_JID))
	c_contacts := C.malloc(C.size_t(n) * C.size_t(C.sizeof_Contact))

	jidsList := unsafe.Slice((*C.JID)(c_jids), n)
	contactList := unsafe.Slice((*C.Contact)(c_contacts), n)

	i := 0
	for jid, contact := range contacts {
		jidsList[i] = jidToC(jid)
		contactList[i] = contactToC(contact)
		i++
	}

	result := C.ContactsMapResult{
		jids:     (*C.JID)(c_jids),
		contacts: (*C.Contact)(c_contacts),
		size:     C.uint32_t(n),
	}
	return result
}

//export C_Disconnect
func C_Disconnect() {
	client.Disconnect()
}

func main() {} // Required for CGO
