use secp256k1::{PublicKey, SecretKey};
use tracing::log;

use crate::integration::vault_core::{VaultCore, VaultCoreInterface};

const NOSTR_IDENTITY_KEY: &str = "nostr_identity_key";

/// Generates a new Nostr ID and returns it along with the secret and public keys.
///
/// The Nostr ID is a base58-encoded string of the public key.
///
/// # Returns
///
/// A tuple containing:
/// - The Nostr ID as a `String`
/// - A tuple of `(SecretKey, PublicKey)`
pub fn generate_nostr_id() -> (String, (SecretKey, PublicKey)) {
    let mut rng = secp256k1::rand::rng();
    let secret_key = SecretKey::new(&mut rng);
    let public_key = secret_key.public_key();
    let nostr = bs58::encode(public_key.serialize()).into_string();

    (nostr, (secret_key, public_key))
}

impl VaultCore {
    /// Initialize the Nostr ID if it's not found.
    /// - return: `(Nostr ID, secret_key)`
    /// - You can get `Public Key` by just `base58::decode(nostr)`
    pub async fn load_nostr_pair(&self) -> (String, String) {
        match self
            .read_secret(NOSTR_IDENTITY_KEY)
            .await
            .expect("Failed to read Nostr ID from vault")
        {
            Some(data) => {
                let nostr = data["nostr"].as_str().unwrap().to_string();
                let secret_key = data["secret_key"].as_str().unwrap().to_string();
                (nostr, secret_key)
            }
            None => {
                log::debug!("Nostr ID not found in vault, generating new one...");
                let (nostr, (secret_key, _)) = generate_nostr_id();
                let data = serde_json::json!({
                    "nostr": nostr,
                    "secret_key": secret_key.display_secret().to_string(),
                })
                .as_object()
                .unwrap()
                .clone();

                self.write_secret(NOSTR_IDENTITY_KEY, Some(data.clone()))
                    .await
                    .expect("Failed to write Nostr ID to vault");
                (nostr, secret_key.display_secret().to_string())
            }
        }
    }

    /// Initialize the Nostr ID and return it along with the secret key.
    pub async fn load_nostr_peerid(&self) -> String {
        let (id, _sk) = self.load_nostr_pair().await;
        id
    }

    /// Initialize the Nostr ID and return it along with the secret key.
    pub async fn load_nostr_secp_pair(&self) -> secp256k1::Keypair {
        let (_, sk) = self.load_nostr_pair().await;
        sk.parse().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use secp256k1::Message;

    use super::*;

    // TODO use mock vault core for testing
    // #[tokio::test]
    // async fn test_init() {
    //     let id = init().await;
    //     println!("Nostr ID: {:?}", id.0);
    //     println!("Secret Key: {:?}", id.1); // private key
    // }

    #[test]
    fn test_generate_nostr_id() {
        let (nostr, keypair) = generate_nostr_id();
        println!("nostr: {nostr:?}");
        println!("keypair: {keypair:?}");
        let secret_key = keypair.0;
        let public_key = keypair.1;

        let nostr_decode = bs58::decode(&nostr).into_vec().unwrap();
        assert_eq!(nostr_decode, public_key.serialize().to_vec());
        assert_eq!(PublicKey::from_slice(&nostr_decode).unwrap(), public_key);
        // verify
        let message = Message::from_digest([0xab; 32]);
        let sig = secret_key.sign_ecdsa(message);
        assert_eq!(public_key.verify(message, &sig), Ok(()));
    }
}
