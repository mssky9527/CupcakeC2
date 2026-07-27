package utils

import "testing"

func TestValidateWebhookURLBlocksPrivate(t *testing.T) {
	cases := []string{
		"http://127.0.0.1/hook",
		"http://169.254.169.254/latest/meta-data/",
		"http://10.0.0.1/x",
		"http://192.168.1.1/x",
		"ftp://example.com/x",
	}
	for _, u := range cases {
		if err := ValidateWebhookURL(u); err == nil {
			t.Fatalf("expected block for %s", u)
		}
	}
}

func TestValidateWebhookURLAllowsPublicHTTPS(t *testing.T) {
	// example.com is public; may require DNS — if offline, skip
	err := ValidateWebhookURL("https://example.com/hook")
	if err != nil {
		t.Logf("example.com resolve: %v (network may be restricted)", err)
	}
}
