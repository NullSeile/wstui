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
	JID jid;
	const char* name;
} ContactEntry;

typedef struct {
	ContactEntry* entries;
	uint32_t size;
} GetContactsResult;

typedef struct {
	char* id;
	JID chat;
	JID sender;
	int64_t timestamp;
	bool isFromMe;
	char* quoteID;
	uint16_t readBy;
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
	JID id;
	char* const* messageIDs;
	size_t size;
} ReceiptEvent;

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
	"os"
	"path/filepath"
	"slices"
	"sort"
	"time"
	"unsafe"

	"google.golang.org/protobuf/proto"

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
	LOG_LEVEL(4, msg, args...)
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
	LOG_DEBUG(msg, args...)
}

func (l *WrLogger) Sub(module string) waLog.Logger {
	return &WrLogger{}
}

// GetSelfId returns the current user's JID string for comparison (e.g. broadcast sender).
func GetSelfId(client *whatsmeow.Client) string {
	if client == nil || client.Store == nil || client.Store.ID == nil {
		return ""
	}
	return StrFromJid(*client.Store.ID)
}

// GetChatId returns the normalized chat id (conversation key): LID→PN, broadcast→per-sender, status as-is.
func GetChatId(client *whatsmeow.Client, chatJid *types.JID, senderJid *types.JID) string {
	if chatJid == nil {
		LOG_WARN("chatJid is nil")
		return ""
	}
	if chatJid.Server == types.BroadcastServer && chatJid.User == "status" {
		return StrFromJid(*chatJid)
	}
	if chatJid.Server == types.BroadcastServer && chatJid.User != "status" {
		if senderJid != nil {
			userId := GetUserId(client, nil, senderJid)
			if userId == GetSelfId(client) {
				return StrFromJid(*chatJid)
			}
			return userId
		}
	}
	if chatJid.Server == types.HiddenUserServer {
		ctx := context.Background()
		if pChatJid, _ := client.Store.LIDs.GetPNForLID(ctx, *chatJid); !pChatJid.IsEmpty() {
			return StrFromJid(pChatJid)
		}
	}
	return StrFromJid(*chatJid)
}

// GetUserId returns the normalized user/sender id: LID→PN when known; in groups use sender as-is (like nchat).
func GetUserId(client *whatsmeow.Client, chatJid *types.JID, userJid *types.JID) string {
	if userJid == nil {
		LOG_WARN("userJid is nil")
		return ""
	}
	if chatJid != nil && chatJid.Server == types.GroupServer {
		return StrFromJid(*userJid)
	}
	if userJid.Server == types.HiddenUserServer {
		ctx := context.Background()
		if pUserJid, _ := client.Store.LIDs.GetPNForLID(ctx, *userJid); !pUserJid.IsEmpty() {
			return StrFromJid(pUserJid)
		}
	}
	return StrFromJid(*userJid)
}

// Convert Jid to string without any mapping, use with care!
func StrFromJid(jid types.JID) string {
	return jid.User + "@" + jid.Server
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

// contactDisplayName returns the display name for a contact (same order as Rust get_contact_name).
func contactDisplayName(c types.ContactInfo) string {
	if c.FullName != "" {
		return c.FullName
	}
	if c.FirstName != "" {
		return c.FirstName
	}
	if c.PushName != "" {
		return "~ " + c.PushName
	}
	if c.BusinessName != "" {
		return "+ " + c.BusinessName
	}
	return ""
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
	container, err := sqlstore.New(context.Background(), "sqlite3", "file:"+goPath+"?_foreign_keys=on", dbLog)
	if err != nil {
		panic(err)
	}
	deviceStore, _ := container.GetFirstDevice(context.Background())
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
	EventTypeReceipt
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

func ContentToWaE2EMessage(messageType C.uint8_t, messageContent unsafe.Pointer, contextInfo *waE2E.ContextInfo) *waE2E.Message {
	switch messageType {
	case C.uint8_t(MessageTypeText):
		textMsg := (*C.TextMessage)(messageContent)
		text := C.GoString(textMsg.text)
		return &waE2E.Message{
			ExtendedTextMessage: &waE2E.ExtendedTextMessage{
				Text:        &text,
				ContextInfo: contextInfo,
			},
		}

	case C.uint8_t(MessageTypeFile):
		fileMsg := (*C.FileMessage)(messageContent)
		kind := uint8(fileMsg.kind)

		filePath := C.GoString(fileMsg.path)
		data, err := os.ReadFile(filePath)
		if err != nil {
			panic(fmt.Sprintf("read file %s err %#v", filePath, err))
		}
		mimetype := mime.TypeByExtension(filepath.Ext(filePath))

		switch kind {
		case FileTypeImage:
			uploaded, upErr := client.Upload(context.Background(), data, whatsmeow.MediaImage)
			if upErr != nil {
				panic(fmt.Sprintf("upload error %#v", upErr))
			}
			return &waE2E.Message{
				ImageMessage: &waE2E.ImageMessage{
					Caption:       proto.String(C.GoString(fileMsg.caption)),
					URL:           proto.String(uploaded.URL),
					DirectPath:    proto.String(uploaded.DirectPath),
					MediaKey:      uploaded.MediaKey,
					Mimetype:      proto.String(mimetype),
					FileEncSHA256: uploaded.FileEncSHA256,
					FileSHA256:    uploaded.FileSHA256,
					FileLength:    proto.Uint64(uint64(len(data))),
					ContextInfo:   contextInfo,
				},
			}
		case FileTypeDocument:
			uploaded, upErr := client.Upload(context.Background(), data, whatsmeow.MediaDocument)
			if upErr != nil {
				panic(fmt.Sprintf("upload error %#v", upErr))
			}
			fileName := filepath.Base(filePath)
			return &waE2E.Message{
				DocumentMessage: &waE2E.DocumentMessage{
					Caption:       proto.String(C.GoString(fileMsg.caption)),
					URL:           proto.String(uploaded.URL),
					DirectPath:    proto.String(uploaded.DirectPath),
					MediaKey:      uploaded.MediaKey,
					Mimetype:      proto.String(mimetype),
					FileEncSHA256: uploaded.FileEncSHA256,
					FileSHA256:    uploaded.FileSHA256,
					FileLength:    proto.Uint64(uint64(len(data))),
					FileName:      proto.String(fileName),
					ContextInfo:   contextInfo,
				},
			}
		default:
			panic(fmt.Sprintf("Unsupported file type: %v", kind))
		}

	default:
		panic(fmt.Sprintf("Unsupported message type: %d", messageType))
	}
}

func HandleMessage(info types.MessageInfo, msg *waE2E.Message, isSync bool) {
	// Normalize chat and sender ids (LID→PN, broadcast→per-sender) so Rust sees canonical ids.
	if normalizedChat := GetChatId(client, &info.Chat, &info.Sender); normalizedChat != "" {
		if jid, err := types.ParseJID(normalizedChat); err == nil {
			info.Chat = jid
		}
	}
	if normalizedSender := GetUserId(client, &info.Chat, &info.Sender); normalizedSender != "" {
		if jid, err := types.ParseJID(normalizedSender); err == nil {
			info.Sender = jid
		}
	}

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
		readBy:    C.uint16_t(0),
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
		case *events.MarkChatAsRead:
			LOG_DEBUG("MarkChatAsRead %v", evt.JID)

		case *events.AppStateSyncComplete:
			LOG_INFO("AppStateSyncComplete %v", evt)
			if evt.Name == appstate.WAPatchRegular {
				LOG_INFO("AppStateSyncComplete (WAPatchRegular) %v", evt)

				cevent := C.Event{
					kind: C.uint8_t(EventTypeAppStateSyncComplete),
					data: nil,
				}
				C.callEventCallback(eventHandler, &cevent)
			}

		case *events.Message:
			HandleMessage(evt.Info, evt.Message, false)

		case *events.Receipt:

			receiptKind := -1
			if evt.Type == types.ReceiptTypeRead || evt.Type == types.ReceiptTypeReadSelf {
				receiptKind = 0
			}

			if receiptKind != -1 {
				LOG_DEBUG("%#v was read by %s at %s", evt.MessageIDs, evt.SourceString(), evt.Timestamp)
				n := len(evt.MessageIDs)
				cmessageIds := (**C.char)(C.malloc(C.size_t(n) * C.size_t(unsafe.Sizeof(uintptr(0)))))
				messageIds := unsafe.Slice(cmessageIds, len(evt.MessageIDs))
				for i, id := range evt.MessageIDs {
					messageIds[i] = C.CString(id)
				}

				cchatId := jidToC(evt.MessageSource.Chat)

				creceipt := (*C.ReceiptEvent)(C.malloc(C.sizeof_ReceiptEvent))
				creceipt.kind = C.uint8_t(EventTypeReceipt)
				creceipt.id = cchatId
				creceipt.messageIDs = cmessageIds
				creceipt.size = C.size_t(n)

				cevent := C.Event{
					kind: C.uint8_t(EventTypeReceipt),
					data: unsafe.Pointer(creceipt),
				}
				C.callEventCallback(eventHandler, &cevent)
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
	code, err := client.PairPhone(context.Background(), goPhone, true, whatsmeow.PairClientChrome, "Chrome (Linux)")
	if err != nil {
		panic(err)
	}
	cCode := C.CString(code)
	return cCode
}

//export C_SendMessage
func C_SendMessage(cjid C.JID, messageType C.uint8_t, messageContent unsafe.Pointer, quoteId *C.char, quoteSender C.JID) {
	jid := cToJid(cjid)

	contextInfo := &waE2E.ContextInfo{}
	if quoteId != nil {
		id := C.GoString(quoteId)
		contextInfo.StanzaID = &id
		sender := C.GoString(quoteSender)
		contextInfo.Participant = &sender
	}

	message := ContentToWaE2EMessage(messageType, messageContent, contextInfo)

	sendResponse, err := client.SendMessage(context.Background(), jid, message)
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
		HandleMessage(messageInfo, message, false)
	}
}

// TODO: Free the memory allocated for C.JID and C.Contact

//export C_GetContacts
func C_GetContacts() C.GetContactsResult {
	ctx := context.Background()
	var entries []C.ContactEntry

	// Contacts (with LID aliases so group senders keyed by LID resolve to a name).
	contacts, err := client.Store.Contacts.GetAllContacts(ctx)
	if err != nil {
		panic(err)
	}
	for jid, contact := range contacts {
		name := contactDisplayName(contact)
		if name == "" {
			continue
		}
		cName := C.CString(name)
		entries = append(entries, C.ContactEntry{jid: jidToC(jid), name: cName})
		if jid.Server != types.HiddenUserServer {
			if lid, _ := client.Store.LIDs.GetLIDForPN(ctx, jid); !lid.IsEmpty() {
				entries = append(entries, C.ContactEntry{jid: jidToC(lid), name: cName})
			}
		}
	}

	// Groups.
	groups, err := client.GetJoinedGroups(ctx)
	if err != nil {
		panic(err)
	}
	for _, group := range groups {
		entries = append(entries, C.ContactEntry{
			jid:  jidToC(group.JID),
			name: C.CString(group.GroupName.Name),
		})
	}

	n := len(entries)
	c_entries := C.malloc(C.size_t(n) * C.size_t(unsafe.Sizeof(C.ContactEntry{})))
	entryList := unsafe.Slice((*C.ContactEntry)(c_entries), n)
	for i := range n {
		entryList[i] = entries[i]
	}

	return C.GetContactsResult{
		entries: (*C.ContactEntry)(c_entries),
		size:    C.uint32_t(n),
	}
}

//export C_Disconnect
func C_Disconnect() {
	client.Disconnect()
}

func main() {} // Required for CGO
