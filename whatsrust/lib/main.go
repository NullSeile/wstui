package main

/*
#cgo LDFLAGS: -L. -lwhatsmeow
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

typedef void (*QrCallback)(const char*, void*);
typedef void (*EventHandler)(const char*, void*);

typedef struct ContactsMapResult {
	CJID* jids;
	CContact* contacts;
	uint32_t size;
} ContactsMapResult;

typedef void (*TestCallback)(const char*, void*);
static void callTestCallback(TestCallback cb, const char* str, void* user_data) {
	cb(str, user_data);
}

static void callQrCallback(QrCallback cb, const char* code, void* user_data) {
    cb(code, user_data);
}

static void callEventHandler(EventHandler cb, const char* msg, void* user_data) {
	// printf("Calling event handler with msg: %s\n", msg);
    cb(msg, user_data);
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
	"unsafe"

	_ "github.com/mattn/go-sqlite3"
	"go.mau.fi/whatsmeow"
	"go.mau.fi/whatsmeow/proto/waE2E"
	"go.mau.fi/whatsmeow/store/sqlstore"
	"go.mau.fi/whatsmeow/types"
	"go.mau.fi/whatsmeow/types/events"
	waLog "go.mau.fi/whatsmeow/util/log"
)

var client *whatsmeow.Client
var qrChan <-chan whatsmeow.QRChannelItem

var logHandler C.LogHandler

// Logger
type WrLogger struct{}

func (l *WrLogger) Errorf(msg string, args ...any) {
	if logHandler != nil {
		cmsg := C.CString(fmt.Sprintf(msg, args...))
		defer C.free(unsafe.Pointer(cmsg))
		C.callLogInfo(logHandler, cmsg, 0)
	}
}
func (l *WrLogger) Warnf(msg string, args ...any) {
	if logHandler != nil {
		cmsg := C.CString(fmt.Sprintf(msg, args...))
		defer C.free(unsafe.Pointer(cmsg))
		C.callLogInfo(logHandler, cmsg, 1)
	}
}
func (l *WrLogger) Infof(msg string, args ...any) {
	if logHandler != nil {
		cmsg := C.CString(fmt.Sprintf(msg, args...))
		defer C.free(unsafe.Pointer(cmsg))
		C.callLogInfo(logHandler, cmsg, 2)
	}
}
func (l *WrLogger) Debugf(msg string, args ...any) {
	if logHandler != nil {
		cmsg := C.CString(fmt.Sprintf(msg, args...))
		defer C.free(unsafe.Pointer(cmsg))
		C.callLogInfo(logHandler, cmsg, 3)
	}
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
	// fmt.Println("Database path:", goPath)
	container, err := sqlstore.New("sqlite3", "file:"+goPath+"?_foreign_keys=on", dbLog)
	if err != nil {
		panic(err)
	}
	deviceStore, _ := container.GetFirstDevice()
	// clientLog := waLog.Stdout("Client", "DEBUG", true)
	clientLog := &WrLogger{}
	client = whatsmeow.NewClient(deviceStore, clientLog)
}

//export C_Test
func C_Test(callback C.TestCallback, data unsafe.Pointer) {
	str := "Hello from Go!"
	C.callTestCallback(callback, C.CString(str), data)
}

//export C_AddEventHandler
func C_AddEventHandler(handler C.EventHandler, data unsafe.Pointer) {
	client.AddEventHandler(func(evt any) {
		switch evt.(type) {
		case *events.Message:
			evt := evt.(*events.Message)
			msg := evt.Message
			// info := evt.Info

			// chat := info.Chat
			// sender := info.Sender
			// is_group := info.IsGroup

			if msg.Conversation != nil {
				cmsg := C.CString(msg.GetConversation())
				defer C.free(unsafe.Pointer(cmsg))
				C.callEventHandler(handler, cmsg, data)
			}

			// evt.Info.MessageSource
			//
			// cmsg := C.CString(evt.Message.GetConversation())
			// defer C.free(unsafe.Pointer(cmsg))
			// C.callEventHandler(handler, cmsg, data)

			// case *events.HistorySync:
			// 	evt := evt.(*events.HistorySync)
			// 	// fmt.Println("History sync event:", evt)
			// 	conversations := evt.Data.GetConversations()
			// 	for _, conversation := range conversations {
			// 		messages := conversation.GetMessages()
			// 		for _, messageWeb := range messages {
			// 			msg := messageWeb.Message.GetMessage()
			// 		}
			//
			// 	}
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
