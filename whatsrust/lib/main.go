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
	char* quoteID;
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
		text := msg.GetExtendedTextMessage().GetText()
		context_info := msg.GetExtendedTextMessage().GetContextInfo()
		if context_info != nil {
			id := context_info.GetStanzaID()
			if id != "" {
				cinfo.quoteID = C.CString(id)
			}
		}
		ctext := C.CString(text)
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

	if msg.ImageMessage != nil {
		img := msg.GetImageMessage()
		if img == nil {
			LOG_ERROR("ImageMessage is nil")
			return
		}

		ext := ExtensionByType(img.GetMimetype(), ".jpg")
		caption := img.GetCaption()

		context_info := msg.GetExtendedTextMessage().GetContextInfo()
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

		// imageData, err := client.DownloadMediaWithPath(img.GetDirectPath(), img.GetFileEncSHA256(), img.GetFileSHA256(), img.GetMediaKey(), getSize(img), mediaType, mediaTypeToMMSType[mediaType])
		// _, _ = DownloadFromFileId(client, fileId)

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
func C_DownloadFile(fileId *C.char) {
	goFileId := C.GoString(fileId)
	DownloadFromFileId(client, goFileId)
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
