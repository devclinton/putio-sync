// The putio-sync plugin ABI for Go and TinyGo (PLUGINS.md, "WebAssembly plugins"):
// JSON in and out through linear memory, one host import for the whole host API.
package main

import (
	"encoding/json"
	"unsafe"
)

//go:wasmimport putio call
func hostCall(ptr, size uint32) uint32

//go:wasmimport putio reply
func hostReply(ptr uint32)

//go:wasmimport putio fail
func hostFail(ptr, size uint32)

// Buffers handed to the host stay referenced here until putio_free, so the garbage
// collector leaves them alone.
var buffers = map[uint32][]byte{}

//go:wasmexport putio_alloc
func putioAlloc(size uint32) uint32 {
	if size == 0 {
		size = 1
	}
	buf := make([]byte, size)
	ptr := uint32(uintptr(unsafe.Pointer(&buf[0])))
	buffers[ptr] = buf
	return ptr
}

//go:wasmexport putio_free
func putioFree(ptr, size uint32) {
	delete(buffers, ptr)
}

// take reads and frees a buffer the host filled
func take(ptr, size uint32) []byte {
	buf := buffers[ptr]
	delete(buffers, ptr)
	if uint32(len(buf)) < size {
		return nil
	}
	return buf[:size]
}

// args decodes a function's JSON argument array
func args(ptr, size uint32, into ...any) bool {
	var raw []json.RawMessage
	if json.Unmarshal(take(ptr, size), &raw) != nil {
		return false
	}
	for i, v := range into {
		if i < len(raw) && json.Unmarshal(raw[i], v) != nil {
			return false
		}
	}
	return true
}

// result hands a value back to the host; nil means "no answer"
func result(v any) uint64 {
	if v == nil {
		return 0
	}
	text, err := json.Marshal(v)
	if err != nil {
		fail(err.Error())
		return 0
	}
	ptr := putioAlloc(uint32(len(text)))
	copy(buffers[ptr], text)
	return uint64(ptr)<<32 | uint64(len(text))
}

// call runs one host API call; the error is the host's message
func call(fn string, out any, a ...any) string {
	if a == nil {
		a = []any{}
	}
	req, _ := json.Marshal(map[string]any{"fn": fn, "args": a})
	size := hostCall(uint32(uintptr(unsafe.Pointer(&req[0]))), uint32(len(req)))
	buf := make([]byte, size+1)
	hostReply(uint32(uintptr(unsafe.Pointer(&buf[0]))))
	var reply struct {
		Ok    json.RawMessage `json:"ok"`
		Error string          `json:"error"`
	}
	if json.Unmarshal(buf[:size], &reply) != nil {
		return "bad reply"
	}
	if reply.Error != "" {
		return reply.Error
	}
	if out != nil {
		json.Unmarshal(reply.Ok, out)
	}
	return ""
}

func logInfo(msg string, fields map[string]any) { call("log.info", nil, msg, fields) }

func fail(msg string) {
	b := []byte(msg)
	if len(b) == 0 {
		b = []byte("failed")
	}
	hostFail(uint32(uintptr(unsafe.Pointer(&b[0]))), uint32(len(b)))
}

// main is not run: the plugin is built as a reactor (-buildmode=c-shared)
func main() {}
