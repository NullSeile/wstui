package main

import (
	"encoding/json"
	"go.mau.fi/whatsmeow"
	"os"
)

const (
	MediaImage    whatsmeow.MediaType = "WhatsApp Image Keys"
	MediaVideo    whatsmeow.MediaType = "WhatsApp Video Keys"
	MediaAudio    whatsmeow.MediaType = "WhatsApp Audio Keys"
	MediaDocument whatsmeow.MediaType = "WhatsApp Document Keys"
	MediaHistory  whatsmeow.MediaType = "WhatsApp History Keys"
	MediaAppState whatsmeow.MediaType = "WhatsApp App State Keys"

	MediaLinkThumbnail whatsmeow.MediaType = "WhatsApp Link Thumbnail Keys"
)

var mediaTypeToMMSType = map[whatsmeow.MediaType]string{
	MediaImage:    "image",
	MediaAudio:    "audio",
	MediaVideo:    "video",
	MediaDocument: "document",
	MediaHistory:  "md-msg-hist",
	MediaAppState: "md-app-state",

	MediaLinkThumbnail: "thumbnail-link",
}

type downloadableMessageWithLength interface {
	whatsmeow.DownloadableMessage
	GetFileLength() uint64
}

type downloadableMessageWithSizeBytes interface {
	whatsmeow.DownloadableMessage
	GetFileSizeBytes() uint64
}

func getSize(msg whatsmeow.DownloadableMessage) int {
	switch sized := msg.(type) {
	case downloadableMessageWithLength:
		return int(sized.GetFileLength())
	case downloadableMessageWithSizeBytes:
		return int(sized.GetFileSizeBytes())
	default:
		return -1
	}
}

var downloadInfoVersion = 1 // bump version upon any struct change
type DownloadInfo struct {
	Version int `json:"Version_int"`
	// Url        string `json:"Url_string"`
	DirectPath string `json:"DirectPath_string"`

	TargetPath string              `json:"TargetPath_string"`
	MediaKey   []byte              `json:"MediaKey_arraybyte"`
	MediaType  whatsmeow.MediaType `json:"MediaType_MediaType"`
	Size       int                 `json:"Size_int"`

	FileEncSha256 []byte `json:"FileEncSha256_arraybyte"`
	FileSha256    []byte `json:"FileSha256_arraybyte"`
}

func DownloadableMessageToFileId(client *whatsmeow.Client, msg whatsmeow.DownloadableMessage, targetPath string) string {
	var info DownloadInfo
	info.Version = downloadInfoVersion

	info.TargetPath = targetPath
	info.MediaKey = msg.GetMediaKey()
	info.Size = getSize(msg)
	info.FileEncSha256 = msg.GetFileEncSHA256()
	info.FileSha256 = msg.GetFileSHA256()
	info.DirectPath = msg.GetDirectPath()

	info.MediaType = whatsmeow.GetMediaType(msg)
	if len(info.MediaType) == 0 {
		return ""
	}

	bytes, err := json.Marshal(info)
	if err != nil {
		return ""
	}

	str := string(bytes)

	return str
}

const (
	FileStatusNone = iota - 1
	FileStatusDownloaded
	FileStatusDownloadFailed
)

// TODO: Implement URL download
func DownloadFromFileInfo(client *whatsmeow.Client, info DownloadInfo) ([]byte, error) {
	return client.DownloadMediaWithPath(info.DirectPath, info.FileEncSha256, info.FileSha256, info.MediaKey, info.Size, info.MediaType, mediaTypeToMMSType[info.MediaType])
}
func FileIdToDownloadInfo(fileId string) (DownloadInfo, error) {
	var info DownloadInfo
	err := json.Unmarshal([]byte(fileId), &info)
	if err != nil {
		return info, err
	}
	if info.Version != downloadInfoVersion {
		return info, err
	}
	return info, nil
}
func DownloadFromFileId(client *whatsmeow.Client, fileId string) (string, int) {
	info, err := FileIdToDownloadInfo(fileId)
	if err != nil {
		return "", FileStatusDownloadFailed
	}

	targetPath := info.TargetPath
	filePath := ""
	fileStatus := FileStatusNone

	// download if not yet present
	if _, statErr := os.Stat(targetPath); os.IsNotExist(statErr) {
		data, err := DownloadFromFileInfo(client, info)
		if err != nil {
			fileStatus = FileStatusDownloadFailed
		} else {
			file, err := os.Create(targetPath)
			defer file.Close()
			if err != nil {
				fileStatus = FileStatusDownloadFailed
			} else {
				_, err = file.Write(data)
				if err != nil {
					fileStatus = FileStatusDownloadFailed
				} else {
					filePath = targetPath
					fileStatus = FileStatusDownloaded
				}
			}
		}
	} else {
		filePath = targetPath
		fileStatus = FileStatusDownloaded
	}

	return filePath, fileStatus
}
