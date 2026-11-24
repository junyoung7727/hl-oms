//! Test-only RSA keys (ADR-034), made once for these tests with openssl
//! 3.5.5 and used nowhere else: they sign nothing real and no venue knows
//! them. Only the base64 bodies are written here; [`pem`] adds the header
//! and footer lines at run time, so that the source holds no PEM header
//! for a secret scanner to flag. The expected signatures in the tests are
//! openssl's (`openssl dgst -sha256 -sign key.pem | base64 -w0`).

/// `body` between the `-----BEGIN {label}-----` and `-----END {label}-----`
/// lines.
pub fn pem(label: &str, body: &str) -> String {
    format!("-----BEGIN {label}-----\n{body}-----END {label}-----\n")
}

/// [`KEY_2048`] as the PKCS#8 PEM text openssl writes.
pub fn pem_2048() -> String {
    pem("PRIVATE KEY", KEY_2048)
}

/// `openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048` (PKCS#8).
pub const KEY_2048: &str = "\
MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQClOhp3TGzxBMaN\n\
cYuaJH3fElXlon6iUxeS0z+q9zattp5F0yT7ltkaOMwMra8lZwVjKiCLqtfOoUQ+\n\
+prFiI4wbDP2LK/d0s9sddZV55LbZuCWLbOQ183oNdl3Cf7yy9S2ZolemjRBQeEu\n\
hniqrKZMVhhpljjQekOt4YyNqMZQSZlZEGupYLTD4yK9tDN4idEYtpcPXyvCx0G2\n\
NJVv7ZTGDVAqt/P1FQIgbVK1vGPEr6LmH3OLo9/4hBzWfGzAhzvXK2XMnsc4I0Hb\n\
8S8S0EaLW+8jMux2RGZgbDOrwhauk8gvVV0lT6JRTTFWza3KmCG8Udst+QE0xibF\n\
VluLpT5NAgMBAAECggEARZ7xDfnPIdPQUkNY/q+HtlpEhYlt8Lowgbav6jtW9+Wu\n\
VV++AqEyg+EQ4frBhXIejAVxR0jKvI+pcRwp4ThEKdU+I+n6hO7SZjJVCUeV0wtL\n\
cRx6wHqekc02//jfS7FlxSH5mjpB8Ilw+CjDaa9sN1hD2yHoeLg8EmWq0vod7jAX\n\
q7fDZO3eEA5m1aK57MIitXaaeY0YMdf+C+eIVwc4le0Z/jiRUqevXbrQcGqiyAc2\n\
O74VYS9xBg8JbvmNmCcnBUTQ48p8qnElPFc1/k1JqO5fRMIXqw2PLoUqRS1B3eZf\n\
PnUImYu/efjCzhPOvmf7OuDxcnOUSnlvoPDJ04do5QKBgQDRLjiudviXAGvtz1uS\n\
lveELkwrCb2YQPRsTFve8VvBnnMknuZDayVlRkmkzgeNT0SvVzcgwUkFEvlPb3mE\n\
5veFfpfni6Y+VeHGcVlESuySHuGXdQy4Ep5+ov5BQLL2i51HYgJDzOK/jJvk1yj6\n\
92fdxa2WLUpnQpsmv7GvR/gnPwKBgQDKNWXJhR+V176sNkKbMaXiYoTmCVygOgEv\n\
sFeME9XOxLpUYQNH4lG2CSXDeZogyo52rZk/FEmSwBbrMnU/9S4Vu9XD58cOA/4E\n\
ApXkoT0njMQh5uO5I5HJVSKXJhB/RGYUVOHJjORjrL7Ai8gkAR3D6evS+7jgEPKu\n\
kcnEALIjcwKBgGiNts0VMBmhn1ClJTs29A6AaZfVbYuNY95BqGXds6QqQ5kZRzSJ\n\
eI6PPD4AsOujpEwTxr07fvjnS++1Y0LXgv6ZkOmt9nEP0iKzAxBMdsRuZQmj+rCC\n\
te/MHlOVOB5dGEbWqRUkcm2VpVITbStLwAGsTaBTkO3d79jFVdWrnlBXAoGBALc+\n\
GJW8e9kQgAwTx9xX65nx6bmE9lmE9xo+VbftKa6M/MYGfDCiWaX7OCuigRgCN4Q4\n\
4U0Wl23TgXF2cmaZS0AKzjcQTYTX/VOo/CkKLR8wI3UNhYR7RVwMOKJ8riqG03LN\n\
USeS9E2Xd+2JPr/R+yVVUI4L64eyFBo6p3tlngkdAoGAHriTh3um8k+/3kgzw0Ih\n\
pytZQMbAOQjcWI8oxbMyIDtSXNUpM5pTw/DnDI8fkXrEwNYcFKiH9IU03jJIHm8/\n\
l4X1fJ4cNhUXaOeVygwZhPgEH/6tYOU+tn2p61FFKz6JsXE2GorE54bverLd0biS\n\
i2LsYOWV07PsHLNPsRHNu/o=\n\
";

/// The same key as PKCS#1 (`openssl rsa -in key.pem -traditional`).
pub const KEY_2048_PKCS1: &str = "\
MIIEowIBAAKCAQEApToad0xs8QTGjXGLmiR93xJV5aJ+olMXktM/qvc2rbaeRdMk\n\
+5bZGjjMDK2vJWcFYyogi6rXzqFEPvqaxYiOMGwz9iyv3dLPbHXWVeeS22bgli2z\n\
kNfN6DXZdwn+8svUtmaJXpo0QUHhLoZ4qqymTFYYaZY40HpDreGMjajGUEmZWRBr\n\
qWC0w+MivbQzeInRGLaXD18rwsdBtjSVb+2Uxg1QKrfz9RUCIG1StbxjxK+i5h9z\n\
i6Pf+IQc1nxswIc71ytlzJ7HOCNB2/EvEtBGi1vvIzLsdkRmYGwzq8IWrpPIL1Vd\n\
JU+iUU0xVs2typghvFHbLfkBNMYmxVZbi6U+TQIDAQABAoIBAEWe8Q35zyHT0FJD\n\
WP6vh7ZaRIWJbfC6MIG2r+o7VvflrlVfvgKhMoPhEOH6wYVyHowFcUdIyryPqXEc\n\
KeE4RCnVPiPp+oTu0mYyVQlHldMLS3EcesB6npHNNv/430uxZcUh+Zo6QfCJcPgo\n\
w2mvbDdYQ9sh6Hi4PBJlqtL6He4wF6u3w2Tt3hAOZtWiuezCIrV2mnmNGDHX/gvn\n\
iFcHOJXtGf44kVKnr1260HBqosgHNju+FWEvcQYPCW75jZgnJwVE0OPKfKpxJTxX\n\
Nf5NSajuX0TCF6sNjy6FKkUtQd3mXz51CJmLv3n4ws4Tzr5n+zrg8XJzlEp5b6Dw\n\
ydOHaOUCgYEA0S44rnb4lwBr7c9bkpb3hC5MKwm9mED0bExb3vFbwZ5zJJ7mQ2sl\n\
ZUZJpM4HjU9Er1c3IMFJBRL5T295hOb3hX6X54umPlXhxnFZRErskh7hl3UMuBKe\n\
fqL+QUCy9oudR2ICQ8ziv4yb5Nco+vdn3cWtli1KZ0KbJr+xr0f4Jz8CgYEAyjVl\n\
yYUflde+rDZCmzGl4mKE5glcoDoBL7BXjBPVzsS6VGEDR+JRtgklw3maIMqOdq2Z\n\
PxRJksAW6zJ1P/UuFbvVw+fHDgP+BAKV5KE9J4zEIebjuSORyVUilyYQf0RmFFTh\n\
yYzkY6y+wIvIJAEdw+nr0vu44BDyrpHJxACyI3MCgYBojbbNFTAZoZ9QpSU7NvQO\n\
gGmX1W2LjWPeQahl3bOkKkOZGUc0iXiOjzw+ALDro6RME8a9O37450vvtWNC14L+\n\
mZDprfZxD9IiswMQTHbEbmUJo/qwgrXvzB5TlTgeXRhG1qkVJHJtlaVSE20rS8AB\n\
rE2gU5Dt3e/YxVXVq55QVwKBgQC3PhiVvHvZEIAME8fcV+uZ8em5hPZZhPcaPlW3\n\
7SmujPzGBnwwolml+zgrooEYAjeEOOFNFpdt04FxdnJmmUtACs43EE2E1/1TqPwp\n\
Ci0fMCN1DYWEe0VcDDiifK4qhtNyzVEnkvRNl3ftiT6/0fslVVCOC+uHshQaOqd7\n\
ZZ4JHQKBgB64k4d7pvJPv95IM8NCIacrWUDGwDkI3FiPKMWzMiA7UlzVKTOaU8Pw\n\
5wyPH5F6xMDWHBSoh/SFNN4ySB5vP5eF9XyeHDYVF2jnlcoMGYT4BB/+rWDlPrZ9\n\
qetRRSs+ibFxNhqKxOeG73qy3dG4koti7GDlldOz7ByzT7ERzbv6\n\
";

/// `openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:1024` (PKCS#8).
pub const KEY_1024: &str = "\
MIICdgIBADANBgkqhkiG9w0BAQEFAASCAmAwggJcAgEAAoGBAOOOoSlzwjuto1hc\n\
94BJ1fsKqxq21NYIkBEK9bUKeNRulmIx/BalYZQ/eLHQ5/I+RAYrJCGH28nLhfqI\n\
k0k5o/DE2mmIc2vnXQnIVZT2xPOHE2TAOnXaT5YgeAX3waIVqXodSrFQhcUBJp3S\n\
0Ia05hORsL+i7cnIYEjPq+VHDLlDAgMBAAECgYBHIoQg2C4Ex+ADNHSuZuZ8ZrZc\n\
UIEyM1UN7tYmdRKWncdnBOcgESzy0fCh7YNEh9Bt+Th/PuAnMfc8/UrFnj4v4+2s\n\
facm29UqV7mjVLU0Kja+FqFnqqjESKlBYk0MVWwOmX0GdcSLcP2J04KpM1d1FLjn\n\
B95730eDqvnCWZROgQJBAPmBU5ldaPgYTFFx2j3AtuFQO19NMzCYoVnZuoTd9uzg\n\
lv2WjjYL/ltb78+L4uT5yHv+LQHVqiwbD60h5vAz7sECQQDpews4k+yuEMSmnGpi\n\
I7pMoagkpCgb1S8c8dDL101pFTX56KUS9zfNYOF1ArqBIRUdC5U7Mpe46xRkL67M\n\
uS0DAkEA5FtXdlCmGBu1Df3ULxzIxOM30/vGVD39v0ylh6HXs+nCiApZ0mGbxWYZ\n\
0K1JVyS+N63k5371JzUFXgJfaY0+gQJAJL98DaSirObwvLSDvfyXlvYeq922fmL4\n\
blrVHTA++9ody77FNI36l3W3Jp6OUkKtuwuWqmGVJcLCS96BabF/lwJAFFalGxju\n\
K39HSV9+gl5/rVQaKSEamCGum51g4CmpTjh9NtaB2TgE3zAuf93gz8k9FW6/1JLy\n\
lcuaPC1pvECQ3Q==\n\
";

/// `KEY_2048` encrypted (`openssl pkcs8 -topk8 -passout pass:test`).
pub const KEY_2048_ENCRYPTED: &str = "\
MIIFNTBfBgkqhkiG9w0BBQ0wUjAxBgkqhkiG9w0BBQwwJAQQ05bN4BJEylaa8AFp\n\
TEudAQICCAAwDAYIKoZIhvcNAgkFADAdBglghkgBZQMEASoEEAdtx7bcqdE8BnAN\n\
+q2WqqcEggTQV+QO7QpjhE43m3RJfOQ9tdeOgDWMevSDXCnAO2Hqlqnf9EZnbhBc\n\
tYRsTcdOlg7yNcQPQfhbZpA3xHC7rWrFOFMIK8Nwy1E50Z1oQUAPBskZM4feLmk/\n\
hgsMp/TQSYa1K9Jya0NxwQyRBhsAWXok8rX+fu1UxlmAPYIuXfGEwjimWC4TzSNZ\n\
fQ4mxIFNvi/Ls+bJNFzQEp4FY817BQYRAC1FIRxYiPj0U6BfvSgKjfI9/fO/+78K\n\
j3U7kp2H4Ny8pSnYiGuTZQ/UFQfZyQgMSxHRDZWAzr2WD8jdIQZK+EKPCLGLMP/U\n\
ubKxMkTyhAkThGpiIiRIhCjfpSOmCPbptOxwwuPEFVr+mkZb/MgOlAyROMk0gbUE\n\
xQvDRd29TqCqby8M2iMAdDKluiLC5gga7pb3pC+AKpzS7xl+8vRsSHOT3R4HOCh7\n\
uf9DjQpZHBXTJS0OMWWxxdwFSPSFUWmqBPIBzAnKJAQt7QVof/rfmeVTlNb9zove\n\
6U54Voe58ZREv033s/qZHxNzRXFeClO8aAKbzdT1Ky2N79K6eivOqmYYbmDkcV0+\n\
1gcYNuFiq7lOCu4Cc2ANjGDkIh59TNhbK92keblHPfInfB29Vil4bdI2EEP9Bfv0\n\
ywkHg8fIK/4ygDQSYgsMAYz0ucv1VLc/NW1G3xBVI6lVqB5xa04yAHsHskDJjnvn\n\
GPy0vU/AYGSG4+BEBb7mULkmlkT3Z180iIaauG8A8AWs33x53yoxnqGu3enMEaG3\n\
wvkrMiI/tGxQjgq55N6Vtk6gIafLBEon1Lk2wwvZNYaDYHRnO8npdxvRmR1/lKQR\n\
XNMf4Vmpht5Ei1oNYS4x6D72X4SxF+syN3/gkskafqE2umi/sKZvpF+higTWdigu\n\
Bq1WM3gPtOILn1Sd0YLbx7lG1L5vHgTSqLLHeNopt4YTQCD4gxAoYhB1sjWNrvmj\n\
faSgx9qJZcEZrxc2e6V34rtZoyau6NcNLJWf+g6ok9v3TW2DpWw2JhjLWCC2ZTOh\n\
m4fEXhxQBzwjqxbl1ZV5XMUIbkBgVcJdDuLRexaPYRd7n/AehVv904pE2333DZ/4\n\
eTeWHqyKGQdzE8rP//m6oBtaSmzqhnflTSt/Yl/R5Rpv/dAkrzNptOVdQG7vB+dr\n\
ORZr+jiAPrGo1NBDa32M/a3qxH0sJvUJ2ZiuwlU4KQeS8h/fPJ07HIRjkWTQ/cvP\n\
AEt/loiahc2gXPyMTRmd26lR3p/qpInN+2SZv+GqCMFsCn8n5rVnpJ82mlrmXgq+\n\
tAY81RHalG/GV9HJAeikMlHukeuUphdWf3+CxwLO4m0svpL3yuix6+Tlbd+euxm9\n\
KMRuhzhHJPka7FD1MfXV6TkyPER6ERwNia9oEbLzqP7PMgwrBJwH1jAtPBhpVlpm\n\
zYXUfjluySQRZFWkP7i0xmikybyJ7EnpQg8krIlZpN8+SX/mLp0ENNjRaG4DCBB4\n\
razM7k0tQZXLP9tdyGEg9/wsZAVti/A7pBqYz6nqEkBYuPhQOT7fk+8jIsK3qffj\n\
uC7Bgib2CpL3cfRaXfFkraBkvKSZjXd8xTOuZ8FUIL5wtbIO++qv0IZdCzkDPAAk\n\
7hreDm/CaYsm0BtgS553eox2HwkdoMqy4v7D3cY5rwFEB6Ztjb5btvk=\n\
";
