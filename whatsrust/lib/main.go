package main

/*
#include <stdlib.h>
#include <stdint.h>
#include <stdbool.h>
#include <stdio.h>

typedef struct {
    char* user;
    uint8_t raw_agent;
    uint16_t device;
    uint16_t integrator;
char* server;
} CJID;

typedef struct {
	bool found;
	const char* first_name;
	const char* full_name;
	const char* push_name;
	const char* business_name;
} CContact;


typedef struct {
	CJID* jids;
	CContact* contacts;
	uint32_t size;
} ContactsMapResult;

typedef struct {
	char* id;
	CJID chat;
	CJID sender;
	int64_t timestamp;
} MessageInfo;

typedef struct {
	MessageInfo info;
	char* message;
} TextMessage;

// typedef struct {
//
// } Chat

typedef void (*QrCallback)(const char*, void*);
static void callQrCallback(QrCallback cb, const char* code, void* user_data) {
	cb(code, user_data);
}

typedef void (*EventHandler)(const TextMessage*, void*);
static void callEventHandler(EventHandler cb, const TextMessage* data, void* user_data) {
    cb(data, user_data);
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
	"time"
	"unsafe"

	_ "github.com/mattn/go-sqlite3"
	"go.mau.fi/whatsmeow"
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
func jidToC(jid types.JID) C.CJID {
	return C.CJID{
		user:       C.CString(jid.User),
		raw_agent:  C.uint8_t(jid.RawAgent),
		device:     C.uint16_t(jid.Device),
		integrator: C.uint16_t(jid.Integrator),
		server:     C.CString(jid.Server),
	}
}

// Convert C JID to Go JID
func cToJid(cjid C.CJID) types.JID {
	return types.JID{
		User:       C.GoString(cjid.user),
		RawAgent:   uint8(cjid.raw_agent),
		Device:     uint16(cjid.device),
		Integrator: uint16(cjid.integrator),
		Server:     C.GoString(cjid.server),
	}
}

// Convert Go ContactInfo to C Contact
func contactToC(contact types.ContactInfo) C.CContact {
	return C.CContact{
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

//export C_NewClient
func C_NewClient(dbPath *C.char) {
	goPath := C.GoString(dbPath)
	// dbLog := waLog.Stdout("Database", "DEBUG", true)
	dbLog := &WrLogger{}
	container, err := sqlstore.New("sqlite3", "file:"+goPath+"?_foreign_keys=on", dbLog)
	if err != nil {
		panic(err)
	}
	deviceStore, _ := container.GetFirstDevice()
	// clientLog := waLog.Stdout("Client", "DEBUG", true)
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

func HandleMessage(info types.MessageInfo, msg *waE2E.Message, handler C.EventHandler, user_data unsafe.Pointer) {
	chat := info.Chat
	sender := info.Sender
	timestamp := info.Timestamp.Unix()

	// LOG_WARN("Chat: %v Sender: %v", chat, sender)

	cinfo := C.MessageInfo{
		id:        C.CString(info.ID),
		chat:      jidToC(chat),
		sender:    jidToC(sender),
		timestamp: C.int64_t(timestamp),
	}

	// LOG_WARN("Message: %v", msg)
	if msg.Conversation != nil {
		// LOG_WARN("Message: %v", msg.Conversation)
		cmsg := C.CString(msg.GetConversation())
		data := C.TextMessage{
			info:    cinfo,
			message: cmsg,
		}

		defer C.free(unsafe.Pointer(cmsg))
		C.callEventHandler(handler, &data, user_data)
	}
	if msg.ExtendedTextMessage != nil {
		// LOG_WARN("ExtendedTextMessage: %v", msg.ExtendedTextMessage)
		text := msg.GetExtendedTextMessage().GetText()
		context_info := msg.GetExtendedTextMessage().GetContextInfo()
		if context_info != nil {
			// LOG_WARN("ContextInfo: %v", context_info)
			// quotedID := context_info.GetStanzaID()
			quoted_message := context_info.GetQuotedMessage()
			if quoted_message != nil {
				// LOG_WARN("QuotedMessage: %v", quoted_message)
				quoted_text := quoted_message.GetConversation()
				text = fmt.Sprintf("(%s) %s", quoted_text, text)
			}
		}
		cmsg := C.CString(text)
		data := C.TextMessage{
			info:    cinfo,
			message: cmsg,
		}
		defer C.free(unsafe.Pointer(cmsg))
		C.callEventHandler(handler, &data, user_data)
	}
}

//export C_AddEventHandler
func C_AddEventHandler(handler C.EventHandler, user_data unsafe.Pointer) {
	client.AddEventHandler(func(evt any) {
		switch evt.(type) {
		case *events.AppStateSyncComplete:
			// C.callSyncComplete
			LOG_ERROR("FINISHEDDDDDDDDD ============")

		case *events.Message:
			evt := evt.(*events.Message)
			HandleMessage(evt.Info, evt.Message, handler, user_data)

		case *events.HistorySync:
			evt := evt.(*events.HistorySync)
			selfJid := *client.Store.ID

			percent := evt.Data.GetProgress()
			LOG_WARN("History sync progress: %d %%", percent)

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

					HandleMessage(*messageInfo, message, handler, user_data)
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

//export C_SendMessage
func C_SendMessage(jid *C.CJID, message *C.char) {
	goJid := cToJid(*jid)
	goMessage := C.GoString(message)

	_, err := client.SendMessage(context.Background(), goJid, &waE2E.Message{
		Conversation: &goMessage,
	})
	if err != nil {
		panic(err)
	}
}

// TODO: Free the memory allocated for C.CJID and C.CContact

//export C_GetAllContacts
func C_GetAllContacts() C.ContactsMapResult {
	contacts, err := client.Store.Contacts.GetAllContacts()
	if err != nil {
		panic(err)
	}

	n := len(contacts)
	c_jids := C.malloc(C.size_t(n) * C.size_t(unsafe.Sizeof(C.CJID{})))
	c_contacts := C.malloc(C.size_t(n) * C.size_t(unsafe.Sizeof(C.CContact{})))

	jidsList := unsafe.Slice((*C.CJID)(c_jids), n)
	contactList := unsafe.Slice((*C.CContact)(c_contacts), n)

	i := 0
	for jid, contact := range contacts {
		jidsList[i] = jidToC(jid)
		contactList[i] = contactToC(contact)
		i++
	}

	result := C.ContactsMapResult{
		jids:     (*C.CJID)(c_jids),
		contacts: (*C.CContact)(c_contacts),
		size:     C.uint32_t(n),
	}
	return result
}

//export C_Disconnect
func C_Disconnect() {
	client.Disconnect()
}

func main() {} // Required for CGO
