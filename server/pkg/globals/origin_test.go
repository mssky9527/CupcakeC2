package globals

import "testing"

func TestOriginAllowedStrictMatch(t *testing.T) {
	cases := []struct {
		origin string
		host   string
		want   bool
	}{
		// Same host + port
		{"http://127.0.0.1:9999", "127.0.0.1:9999", true},
		{"https://localhost:9999", "localhost:9999", true},
		// Default port implied
		{"http://127.0.0.1", "127.0.0.1:80", true},
		{"https://localhost", "localhost:443", true},
		// IPv6 loopback
		{"http://[::1]:9999", "[::1]:9999", true},

		// Malicious subdomains must NOT match
		{"http://localhost.attacker.example", "localhost:9999", false},
		{"https://127.0.0.1.attacker.example", "127.0.0.1:9999", false},
		{"http://localhost.evil.com", "localhost:9999", false},

		// Port mismatch
		{"http://127.0.0.1:8080", "127.0.0.1:9999", false},
		{"http://localhost:443", "localhost:9999", false},

		// Different host
		{"http://evil.com:9999", "127.0.0.1:9999", false},
		{"http://attacker.example", "localhost:9999", false},

		// Empty or malformed
		{"", "127.0.0.1:9999", false},
		{"null", "127.0.0.1:9999", false},
		{"http://", "127.0.0.1:9999", false},

		// Path/query in origin rejected
		{"http://127.0.0.1:9999/evil", "127.0.0.1:9999", false},
		{"http://127.0.0.1:9999?x=1", "127.0.0.1:9999", false},

		// UserInfo in origin rejected
		{"http://user:pass@127.0.0.1:9999", "127.0.0.1:9999", false},
	}

	for _, tc := range cases {
		got := originAllowed(tc.origin, tc.host)
		if got != tc.want {
			t.Errorf("originAllowed(%q, %q) = %v, want %v",
				tc.origin, tc.host, got, tc.want)
		}
	}
}
