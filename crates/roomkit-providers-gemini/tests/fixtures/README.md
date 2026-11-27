`test-only-rsa-pkcs8.der` is a generated, public test key (RSA 2048, PKCS#8
DER). The Vertex cases build their service-account key from it: it signs only
assertions a local fake reads. Never use it for a real service.
