package services

import (
	"bytes"
	"fmt"

	"github.com/Binject/go-donut/donut"
)

// ToShellcodeFromBytes converts raw PE bytes to PIC Shellcode using Donut.
func ToShellcodeFromBytes(raw []byte, arch string) ([]byte, error) {
	// Configure Donut
	donutArch := donut.X64
	if arch == "i386" || arch == "x86" || arch == "386" {
		donutArch = donut.X32
	}

	config := &donut.DonutConfig{
		Arch:       donutArch,
		Type:       donut.DONUT_MODULE_EXE,
		InstType:   donut.DONUT_INSTANCE_PIC,
		Entropy:    donut.DONUT_ENTROPY_NONE,
		Class:      "",
		Method:     "",
		Parameters: "",
		Verbose:    false,
		Bypass:     1, // 1 = DONUT_OPT_BYPASS_NONE. Crucial: prevents Donut from doing VirtualProtect on AMSI/ETW which causes CFG/PatchGuard crashes.
		Thread:     0, // 0 = Execute in current thread. We now manually manage thread creation via NtCreateThreadEx in the loader for better control.
	}

	payload, err := donut.ShellcodeFromBytes(bytes.NewBuffer(raw), config)
	if err != nil {
		return nil, fmt.Errorf("donut conversion failed: %v", err)
	}

	return payload.Bytes(), nil
}
