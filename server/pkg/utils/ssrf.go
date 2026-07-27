package utils

import (
	"fmt"
	"net"
	"net/url"
	"strings"
)

// ValidateWebhookURL rejects private/link-local/metadata targets to mitigate SSRF.
func ValidateWebhookURL(raw string) error {
	u, err := url.Parse(raw)
	if err != nil {
		return fmt.Errorf("invalid URL: %w", err)
	}
	scheme := strings.ToLower(u.Scheme)
	if scheme != "http" && scheme != "https" {
		return fmt.Errorf("webhook scheme must be http or https")
	}
	host := u.Hostname()
	if host == "" {
		return fmt.Errorf("webhook host empty")
	}
	// Block obvious metadata hostnames
	lower := strings.ToLower(host)
	if lower == "metadata.google.internal" || lower == "metadata" ||
		strings.HasSuffix(lower, ".internal") {
		return fmt.Errorf("webhook host blocked")
	}
	ips, err := net.LookupIP(host)
	if err != nil {
		// If DNS fails, still block raw IPs that are private
		if ip := net.ParseIP(host); ip != nil {
			if isBlockedIP(ip) {
				return fmt.Errorf("webhook IP blocked")
			}
			return nil
		}
		return fmt.Errorf("webhook DNS resolve failed: %w", err)
	}
	for _, ip := range ips {
		if isBlockedIP(ip) {
			return fmt.Errorf("webhook resolves to blocked IP %s", ip.String())
		}
	}
	return nil
}

func isBlockedIP(ip net.IP) bool {
	if ip == nil {
		return true
	}
	if ip.IsLoopback() || ip.IsPrivate() || ip.IsLinkLocalUnicast() ||
		ip.IsLinkLocalMulticast() || ip.IsUnspecified() || ip.IsMulticast() {
		return true
	}
	// Cloud metadata
	if ip4 := ip.To4(); ip4 != nil {
		if ip4[0] == 169 && ip4[1] == 254 {
			return true
		}
		// 100.64.0.0/10 CGNAT sometimes used for metadata
		if ip4[0] == 100 && ip4[1] >= 64 && ip4[1] <= 127 {
			return true
		}
	}
	return false
}
