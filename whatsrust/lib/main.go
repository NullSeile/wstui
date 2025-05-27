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
	bool isFromMe;
	char* quoteID;
	bool isRead;
} MessageInfo;

typedef struct {
	char* text;
} TextMessage;

typedef struct {
	uint8_t kind;
	char* path;
	char* fileID;
	char* caption;
} FileMessage;

typedef struct {
	MessageInfo info;
	uint8_t messageType;
	void* message;
} Message;


typedef struct {
	uint8_t kind;
	void* data;
} Event;

typedef void (*EventCallback)(const Event*, void*);
typedef struct {
	EventCallback callback;
	void* user_data;
} EventHandler;
static void callEventCallback(EventHandler hdl, const Event* event) {
	hdl.callback(event, hdl.user_data);
}

typedef void (*QrCallback)(const char*, void*);
static void callQrCallback(QrCallback cb, const char* code, void* user_data) {
	cb(code, user_data);
}

typedef void (*MessageHandlerCallback)(const Message*, bool, void*);
typedef struct {
	MessageHandlerCallback callback;
	void* user_data;
} MessageHandler;
static void callMessageHandler(MessageHandler hdl, bool isSync, const Message* data) {
    hdl.callback(data, isSync, hdl.user_data);
}

typedef void (*HistorySyncCallback)(uint32_t, void*);
typedef struct {
	HistorySyncCallback callback;
	void* user_data;
} HistorySyncHandler;
static void callHistorySync(HistorySyncHandler hdl, uint32_t percent) {
	hdl.callback(percent, hdl.user_data);
}

typedef void (*LogHandlerCallback)(const char*, uint8_t, void*);
typedef struct {
	LogHandlerCallback callback;
	void* user_data;
} LogHandler;
static void callLogInfo(LogHandler hdl, const char* msg, uint8_t level) {
	hdl.callback(msg, level, hdl.user_data);
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
var eventHandler C.EventHandler

func LOG_LEVEL(level int, msg string, args ...any) {
	cmsg := C.CString(fmt.Sprintf(msg, args...))
	defer C.free(unsafe.Pointer(cmsg))
	C.callLogInfo(logHandler, cmsg, C.uint8_t(level))
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
	return C.CString(jid.ToNonAD().String())
	// return C.CString(jid.User + "@" + jid.Server)
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
func C_SetLogHandler(handler C.LogHandler, data unsafe.Pointer) {
	logHandler = C.LogHandler{
		callback:  handler.callback,
		user_data: data,
	}
}

//export C_SetEventHandler
func C_SetEventHandler(handler C.EventHandler, data unsafe.Pointer) {
	eventHandler = C.EventHandler{
		callback:  handler.callback,
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
	EventTypeSyncProgress = iota
	EventTypeAppStateSyncComplete
)

const (
	MessageTypeText = iota
	MessageTypeFile
)

const (
	FileTypeImage = iota
	FileTypeVideo
	FileTypeAudio
	FileTypeDocument
	FileTypeSticker
)

func CMessageInfoToGo(cinfo C.MessageInfo) types.MessageInfo {
	return types.MessageInfo{
		MessageSource: types.MessageSource{
			Chat:     cToJid(cinfo.chat),
			Sender:   cToJid(cinfo.sender),
			IsFromMe: bool(cinfo.isFromMe),
		},
		ID:        C.GoString(cinfo.id),
		Timestamp: time.Unix(int64(cinfo.timestamp), 0),
	}
}

// Theoretically, this should be used for quoting messages, but sending nil as the message
// and only setting the message info works.
func CMessageToWaE2EMessage(cmsg *C.Message) (types.MessageInfo, *waE2E.Message) {
	info := CMessageInfoToGo(cmsg.info)
	return info, nil

	// switch cmsg.messageType {
	// case C.int8_t(MessageTypeText):
	// 	textMsg := (*C.TextMessage)(cmsg.message)
	// 	text := C.GoString(textMsg.text)
	// 	LOG_INFO("Text: %v %s", textMsg.text, text)
	// 	msg := waE2E.Message{
	// 		Conversation: &text,
	// 	}
	// 	return info, &msg
	// case C.int8_t(MessageTypeImage):
	// 	imgMsg := (*C.ImageMessage)(cmsg.message)
	// 	// downloadInfo, err := FileIdToDownloadInfo(C.GoString(imgMsg.fileID))
	// 	// if err != nil {
	// 	// 	panic(err)
	// 	// }
	//
	// 	caption := C.GoString(imgMsg.caption)
	// 	msg := waE2E.Message{
	// 		ImageMessage: &waE2E.ImageMessage{
	// 			Caption: &caption,
	// 		},
	// 	}
	// 	return info, &msg
	// case C.int8_t(MessageTypeVideo):
	// 	vidMsg := (*C.VideoMessage)(cmsg.message)
	//
	// 	caption := C.GoString(vidMsg.caption)
	// 	msg := waE2E.Message{
	// 		VideoMessage: &waE2E.VideoMessage{
	// 			Caption: &caption,
	// 		},
	// 	}
	// 	return info, &msg
	// default:
	// 	return info, nil
	// }
}

func HandleMessage(info types.MessageInfo, msg *waE2E.Message, isSync bool) {
	chat := info.Chat
	sender := info.Sender
	timestamp := info.Timestamp.Unix()

	cinfo := C.MessageInfo{
		id:        C.CString(info.ID),
		chat:      jidToC(chat),
		sender:    jidToC(sender),
		timestamp: C.int64_t(timestamp),
		isFromMe:  C.bool(info.IsFromMe),
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
			messageType: C.uint8_t(MessageTypeText),
			message:     unsafe.Pointer(content),
		}

		C.callMessageHandler(messageHandler, C.bool(isSync), &message)
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
			messageType: C.uint8_t(MessageTypeText),
			message:     unsafe.Pointer(content),
		}
		C.callMessageHandler(messageHandler, C.bool(isSync), &message)
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

		content := (*C.FileMessage)(C.malloc(C.sizeof_FileMessage))
		content.kind = C.uint8_t(FileTypeImage)
		content.path = cpath
		content.fileID = cfileId
		content.caption = ccaption
		defer C.free(unsafe.Pointer(content))

		message := C.Message{
			info:        cinfo,
			messageType: C.uint8_t(MessageTypeFile),
			message:     unsafe.Pointer(content),
		}
		C.callMessageHandler(messageHandler, C.bool(isSync), &message)
	}
	if msg.VideoMessage != nil {
		vid := msg.GetVideoMessage()
		if vid == nil {
			LOG_ERROR("VideoMessage is nil")
			return
		}

		ext := ExtensionByType(vid.GetMimetype(), ".mp4")
		caption := vid.GetCaption()

		context_info := vid.GetContextInfo()
		if context_info != nil {
			id := context_info.GetStanzaID()
			if id != "" {
				cinfo.quoteID = C.CString(id)
			}
		}

		filePath := fmt.Sprintf("videos/%s%s", info.ID, ext)
		fileId := DownloadableMessageToFileId(client, vid, filePath)
		cfileId := C.CString(fileId)
		defer C.free(unsafe.Pointer(cfileId))

		cpath := C.CString(filePath)
		defer C.free(unsafe.Pointer(cpath))

		ccaption := C.CString(caption)
		if caption == "" {
			ccaption = nil
		}
		defer C.free(unsafe.Pointer(ccaption))

		content := (*C.FileMessage)(C.malloc(C.sizeof_FileMessage))
		content.kind = C.uint8_t(FileTypeVideo)
		content.path = cpath
		content.fileID = cfileId
		content.caption = ccaption
		defer C.free(unsafe.Pointer(content))
		message := C.Message{
			info:        cinfo,
			messageType: C.uint8_t(MessageTypeFile),
			message:     unsafe.Pointer(content),
		}
		C.callMessageHandler(messageHandler, C.bool(isSync), &message)
	}
	if msg.AudioMessage != nil {
		audio := msg.GetAudioMessage()
		if audio == nil {
			LOG_ERROR("AudioMessage is nil")
			return
		}

		ext := ExtensionByType(audio.GetMimetype(), ".ogg")

		context_info := audio.GetContextInfo()
		if context_info != nil {
			id := context_info.GetStanzaID()
			if id != "" {
				cinfo.quoteID = C.CString(id)
			}
		}

		filePath := fmt.Sprintf("audios/%s%s", info.ID, ext)
		fileId := DownloadableMessageToFileId(client, audio, filePath)
		cfileId := C.CString(fileId)
		defer C.free(unsafe.Pointer(cfileId))

		cpath := C.CString(filePath)
		defer C.free(unsafe.Pointer(cpath))

		content := (*C.FileMessage)(C.malloc(C.sizeof_FileMessage))
		content.kind = C.uint8_t(FileTypeAudio)
		content.path = cpath
		content.fileID = cfileId
		content.caption = nil
		defer C.free(unsafe.Pointer(content))

		message := C.Message{
			info:        cinfo,
			messageType: C.uint8_t(MessageTypeFile),
			message:     unsafe.Pointer(content),
		}
		C.callMessageHandler(messageHandler, C.bool(isSync), &message)
	}
	if msg.DocumentMessage != nil {
		doc := msg.GetDocumentMessage()
		if doc == nil {
			LOG_ERROR("DocumentMessage is nil")
			return
		}

		caption := doc.GetCaption()

		context_info := doc.GetContextInfo()
		if context_info != nil {
			id := context_info.GetStanzaID()
			if id != "" {
				cinfo.quoteID = C.CString(id)
			}
		}

		filePath := fmt.Sprintf("docs/%s-%s", info.ID, *doc.FileName)
		fileId := DownloadableMessageToFileId(client, doc, filePath)
		cfileId := C.CString(fileId)
		defer C.free(unsafe.Pointer(cfileId))

		cpath := C.CString(filePath)
		defer C.free(unsafe.Pointer(cpath))

		ccaption := C.CString(caption)
		if caption == "" {
			ccaption = nil
		}
		defer C.free(unsafe.Pointer(ccaption))

		content := (*C.FileMessage)(C.malloc(C.sizeof_FileMessage))
		content.kind = C.uint8_t(FileTypeDocument)
		content.path = cpath
		content.fileID = cfileId
		content.caption = ccaption
		defer C.free(unsafe.Pointer(content))

		message := C.Message{
			info:        cinfo,
			messageType: C.uint8_t(MessageTypeFile),
			message:     unsafe.Pointer(content),
		}
		C.callMessageHandler(messageHandler, C.bool(isSync), &message)
	}
	if msg.StickerMessage != nil {
		sticker := msg.GetStickerMessage()
		if sticker == nil {
			LOG_ERROR("StickerMessage is nil")
			return
		}

		ext := ExtensionByType(sticker.GetMimetype(), ".webp")

		context_info := sticker.GetContextInfo()
		if context_info != nil {
			id := context_info.GetStanzaID()
			if id != "" {
				cinfo.quoteID = C.CString(id)
			}
		}

		filePath := fmt.Sprintf("stickers/%s%s", info.ID, ext)
		fileId := DownloadableMessageToFileId(client, sticker, filePath)
		cfileId := C.CString(fileId)
		defer C.free(unsafe.Pointer(cfileId))

		cpath := C.CString(filePath)
		defer C.free(unsafe.Pointer(cpath))

		content := (*C.FileMessage)(C.malloc(C.sizeof_FileMessage))
		content.kind = C.uint8_t(FileTypeSticker)
		content.path = cpath
		content.fileID = cfileId
		content.caption = nil
		defer C.free(unsafe.Pointer(content))

		message := C.Message{
			info:        cinfo,
			messageType: C.uint8_t(MessageTypeFile),
			message:     unsafe.Pointer(content),
		}
		C.callMessageHandler(messageHandler, C.bool(isSync), &message)
	}
}

//export C_DownloadFile
func C_DownloadFile(fileId *C.char, basePath *C.char) C.uint8_t {
	goFileId := C.GoString(fileId)
	goBasePath := C.GoString(basePath)
	status := DownloadFromFileId(client, goFileId, goBasePath)
	return C.uint8_t(status)
}

func AddEventHandlers() {
	client.AddEventHandler(func(rawEvt any) {
		switch evt := rawEvt.(type) {
		case *events.AppStateSyncComplete:
			LOG_ERROR("AppStateStateSyncComplete %v", evt)
			if evt.Name == appstate.WAPatchRegular {
				LOG_ERROR("AppStateStateSyncComplete %v", evt)

				cevent := C.Event{
					kind: C.uint8_t(EventTypeAppStateSyncComplete),
					data: nil,
				}
				C.callEventCallback(eventHandler, &cevent)
			}

		case *events.Message:
			HandleMessage(evt.Info, evt.Message, false)

		case *events.Receipt:
			// LOG_INFO("Receipt: %s %s %s", evt.Type, evt.MessageIDs, evt.SourceString())
			if evt.Type == types.ReceiptTypeRead || evt.Type == types.ReceiptTypeReadSelf {
				LOG_INFO("%#v was read by %s at %s", evt.MessageIDs, evt.SourceString(), evt.Timestamp)
			}

		case *events.HistorySync:
			selfJid := *client.Store.ID

			percent := evt.Data.GetProgress()
			cpercent := (*C.uint8_t)(C.malloc(C.size_t(unsafe.Sizeof(uint8(0)))))
			*cpercent = C.uint8_t(percent)

			cevent := C.Event{
				kind: C.uint8_t(EventTypeSyncProgress),
				data: unsafe.Pointer(cpercent),
			}

			C.callEventCallback(eventHandler, &cevent)

			C.free(unsafe.Pointer(cpercent))

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

					HandleMessage(*messageInfo, message, true)
				}
			}
		}
	})
}

//export C_Connect
func C_Connect(handler C.QrCallback, data unsafe.Pointer) {
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
	} else {
		err := client.Connect()
		if err != nil {
			panic(err)
		}
	}

	AddEventHandlers()
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
		info := CMessageInfoToGo(quoted_msg.info)
		contextInfo.StanzaID = &info.ID
		// contextInfo.QuotedMessage = msg
		quotedSender := info.Sender.String()
		contextInfo.Participant = &quotedSender
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

		LOG_INFO("Message sent: %s %s", messageInfo.ID, messageInfo.Chat)
		HandleMessage(messageInfo, &message, false)
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
