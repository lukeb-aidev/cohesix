// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Provide host-side WPA2-PSK EAPOL helpers for linked CYW43 Wi-Fi.
// Author: Lukas Bower

//! Host-side WPA2-PSK helpers for the linked CYW43 driver model.
//!
//! The linked CYW43 runtime owns SDPCM/SDIO transport. Root owns the policy
//! boundary for secure carrier release: derive keys, answer AP EAPOL, and only
//! publish DHCP/data readiness after PTK/GTK installation has succeeded.

#[cfg(test)]
use aes::cipher::BlockEncrypt;
use aes::cipher::{generic_array::GenericArray, BlockDecrypt, KeyInit};
use aes::Aes128;

use crate::net::MAX_FRAME_LEN;

pub const ETHER_ADDR_LEN: usize = 6;
pub const ETH_HEADER_LEN: usize = 14;
pub const ETH_P_EAPOL: u16 = 0x888e;
pub const WSEC_KEY_PAYLOAD_LEN: usize = 164;
pub const WPA2_PSK_CCMP_RSN_IE: [u8; 22] = [
    0x30, 0x14, 0x01, 0x00, 0x00, 0x0f, 0xac, 0x04, 0x01, 0x00, 0x00, 0x0f, 0xac, 0x04, 0x01, 0x00,
    0x00, 0x0f, 0xac, 0x02, 0x00, 0x00,
];

const EAPOL_HEADER_LEN: usize = 4;
const EAPOL_VERSION_8021X_2004: u8 = 2;
const EAPOL_PACKET_TYPE_START: u8 = 1;
const EAPOL_PACKET_TYPE_KEY: u8 = 3;
const EAPOL_KEY_DESCRIPTOR_RSN: u8 = 2;
const EAPOL_KEY_MIN_BODY_LEN: usize = 95;
const EAPOL_KEY_BODY_DESCRIPTOR_OFFSET: usize = 0;
const EAPOL_KEY_BODY_KEY_INFO_OFFSET: usize = 1;
const EAPOL_KEY_BODY_KEY_LEN_OFFSET: usize = 3;
const EAPOL_KEY_BODY_REPLAY_OFFSET: usize = 5;
const EAPOL_KEY_BODY_NONCE_OFFSET: usize = 13;
const EAPOL_KEY_BODY_RSC_OFFSET: usize = 61;
const EAPOL_KEY_BODY_MIC_OFFSET: usize = 77;
const EAPOL_KEY_BODY_DATA_LEN_OFFSET: usize = 93;
const EAPOL_KEY_BODY_DATA_OFFSET: usize = 95;
const EAPOL_KEY_INFO_KEY_TYPE: u16 = 1 << 3;
const EAPOL_KEY_INFO_INSTALL: u16 = 1 << 6;
const EAPOL_KEY_INFO_ACK: u16 = 1 << 7;
const EAPOL_KEY_INFO_MIC: u16 = 1 << 8;
const EAPOL_KEY_INFO_SECURE: u16 = 1 << 9;
const EAPOL_KEY_INFO_ENCRYPTED_KEY_DATA: u16 = 1 << 12;
const EAPOL_KEY_INFO_KEY_VERSION_MASK: u16 = 0x0007;
const EAPOL_KEY_VERSION_HMAC_SHA1_AES: u16 = 2;
const EAPOL_KEY_INFO_M2: u16 =
    EAPOL_KEY_VERSION_HMAC_SHA1_AES | EAPOL_KEY_INFO_KEY_TYPE | EAPOL_KEY_INFO_MIC;
const EAPOL_KEY_INFO_M4: u16 = EAPOL_KEY_VERSION_HMAC_SHA1_AES
    | EAPOL_KEY_INFO_KEY_TYPE
    | EAPOL_KEY_INFO_MIC
    | EAPOL_KEY_INFO_SECURE;
const EAPOL_KEY_INFO_GROUP_M2: u16 =
    EAPOL_KEY_VERSION_HMAC_SHA1_AES | EAPOL_KEY_INFO_MIC | EAPOL_KEY_INFO_SECURE;
const EAPOL_KEY_INFO_PAIRWISE_RECV_MASK: u16 =
    EAPOL_KEY_INFO_KEY_TYPE | EAPOL_KEY_INFO_INSTALL | EAPOL_KEY_INFO_ACK | EAPOL_KEY_INFO_MIC;
const PAE_GROUP_ADDR: [u8; ETHER_ADDR_LEN] = [0x01, 0x80, 0xc2, 0x00, 0x00, 0x03];
const WPA_EAPOL_REPLY_KEY_LEN: u16 = 0;
const WPA_REPLAY_COUNTER_LEN: usize = 8;
const WPA_NONCE_LEN: usize = 32;
const WPA_KCK_LEN: usize = 16;
const WPA_KEK_LEN: usize = 16;
const WPA_TK_LEN: usize = 16;
const WPA_PTK_LEN: usize = 48;
const WPA_MIC_LEN: usize = 16;
const WSEC_PMK_LEN: usize = 32;
const WSEC_KEY_DATA_LEN: usize = 32;
const WSEC_KEY_INDEX_OFFSET: usize = 0;
const WSEC_KEY_LEN_OFFSET: usize = 4;
const WSEC_KEY_DATA_OFFSET: usize = 8;
const WSEC_KEY_ALGO_OFFSET: usize = 112;
const WSEC_KEY_FLAGS_OFFSET: usize = 116;
const WSEC_KEY_IV_INITIALIZED_OFFSET: usize = 132;
const WSEC_KEY_RXIV_HI_OFFSET: usize = 140;
const WSEC_KEY_RXIV_LO_OFFSET: usize = 144;
const WSEC_KEY_EA_OFFSET: usize = 156;
const HOST_EAPOL_PTK_CANDIDATES: usize = 4;
const CRYPTO_ALGO_AES_CCM: u32 = 4;
const BRCMF_PRIMARY_KEY: u32 = 1 << 1;
const WPA2_PSK_MIN_PASSPHRASE_LEN: usize = 8;
const WPA2_PSK_MAX_PASSPHRASE_LEN: usize = 63;
const WPA2_PSK_HEX_PMK_LEN: usize = 64;
const WPA2_PSK_PBKDF2_ROUNDS: u16 = 4096;
const WPA2_PSK_BLOCK_COUNT: u32 = 2;
const SHA1_BLOCK_LEN: usize = 64;
const SHA1_DIGEST_LEN: usize = 20;
const WPA_PTK_PRF_LABEL: &[u8] = b"Pairwise key expansion\0";
const WPA_SNONCE_LABEL_PREFIX: &[u8] = b"Cohesix host WPA SNonce ";
const HOST_EAPOL_KEY_DATA_MAX_LEN: usize = 256;
const RSN_IE_ID: u8 = 48;
const RSN_VERSION_1: u16 = 1;
const RSN_SUITE_LEN: usize = 4;
const RSN_CIPHER_CCMP: u8 = 4;
const RSN_AKM_PSK: u8 = 2;
const RSN_KDE_OUI: [u8; 3] = [0x00, 0x0f, 0xac];
const RSN_KDE_TYPE_GTK: u8 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostGtkKey {
    pub index: u8,
    pub key_len: usize,
    pub key: [u8; WSEC_KEY_DATA_LEN],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostEapolInstallKeys {
    pub ap_mac: [u8; ETHER_ADDR_LEN],
    pub pairwise_tk: [u8; WPA_TK_LEN],
    pub gtk: Option<HostGtkKey>,
    pub rsc: [u8; 6],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostEapolGroupKeys {
    pub gtk: HostGtkKey,
    pub rsc: [u8; 6],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct HostPtkCandidate {
    valid: bool,
    ap_mac: [u8; ETHER_ADDR_LEN],
    anonce: [u8; WPA_NONCE_LEN],
    snonce: [u8; WPA_NONCE_LEN],
    m1_replay_counter: [u8; WPA_REPLAY_COUNTER_LEN],
    ptk: [u8; WPA_PTK_LEN],
}

impl HostPtkCandidate {
    const fn empty() -> Self {
        Self {
            valid: false,
            ap_mac: [0; ETHER_ADDR_LEN],
            anonce: [0; WPA_NONCE_LEN],
            snonce: [0; WPA_NONCE_LEN],
            m1_replay_counter: [0; WPA_REPLAY_COUNTER_LEN],
            ptk: [0; WPA_PTK_LEN],
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostEapolAction {
    None,
    Inspect {
        proof: HostEapolFrameProof,
    },
    SendM2 {
        len: usize,
    },
    SendM4InstallKeys {
        len: usize,
        keys: HostEapolInstallKeys,
    },
    SendGroupM2InstallGtk {
        len: usize,
        keys: HostEapolGroupKeys,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostEapolState {
    pmk: [u8; WSEC_PMK_LEN],
    ptk: [u8; WPA_PTK_LEN],
    anonce: [u8; WPA_NONCE_LEN],
    snonce: [u8; WPA_NONCE_LEN],
    m1_replay_counter: [u8; WPA_REPLAY_COUNTER_LEN],
    m3_replay_counter: [u8; WPA_REPLAY_COUNTER_LEN],
    group_replay_counter: [u8; WPA_REPLAY_COUNTER_LEN],
    ap_mac: [u8; ETHER_ADDR_LEN],
    rx_packets: u32,
    group_replay_counter_valid: bool,
    ptk_installed: bool,
    gtk_installed: bool,
    m2_sent: bool,
    m4_sent: bool,
    ptk_candidates: [HostPtkCandidate; HOST_EAPOL_PTK_CANDIDATES],
    ptk_candidate_count: usize,
    ptk_candidate_next: usize,
}

impl HostEapolState {
    pub fn new(ssid: &[u8], psk: &[u8]) -> Result<Self, &'static str> {
        let mut pmk = [0u8; WSEC_PMK_LEN];
        fill_wpa2_psk_pmk(ssid, psk, &mut pmk)?;
        Ok(Self {
            pmk,
            ptk: [0; WPA_PTK_LEN],
            anonce: [0; WPA_NONCE_LEN],
            snonce: [0; WPA_NONCE_LEN],
            m1_replay_counter: [0; WPA_REPLAY_COUNTER_LEN],
            m3_replay_counter: [0; WPA_REPLAY_COUNTER_LEN],
            group_replay_counter: [0; WPA_REPLAY_COUNTER_LEN],
            ap_mac: [0; ETHER_ADDR_LEN],
            rx_packets: 0,
            group_replay_counter_valid: false,
            ptk_installed: false,
            gtk_installed: false,
            m2_sent: false,
            m4_sent: false,
            ptk_candidates: [HostPtkCandidate::empty(); HOST_EAPOL_PTK_CANDIDATES],
            ptk_candidate_count: 0,
            ptk_candidate_next: 0,
        })
    }

    pub const fn rx_packets(&self) -> u32 {
        self.rx_packets
    }

    pub const fn secure_complete(&self) -> bool {
        self.m2_sent && self.m4_sent && self.ptk_installed && self.gtk_installed
    }

    pub fn handle_packet(
        &mut self,
        station_mac: [u8; ETHER_ADDR_LEN],
        packet: &[u8],
        tx_frame: &mut [u8; MAX_FRAME_LEN],
    ) -> Result<HostEapolAction, &'static str> {
        if ethernet_ethertype(packet) != Some(ETH_P_EAPOL) {
            return Ok(HostEapolAction::None);
        }
        self.rx_packets = self.rx_packets.saturating_add(1);
        if !packet_dst_allowed(packet, station_mac) {
            return Err("host-eapol-foreign-dst");
        }
        let proof = HostEapolFrameProof::parse(packet)?;
        match proof.message.as_bytes() {
            b"m1" => self.handle_m1(station_mac, packet, proof, tx_frame),
            b"m3" => self.handle_m3(station_mac, packet, proof, tx_frame),
            b"group-key" => self.handle_group_key(station_mac, packet, proof, tx_frame),
            _ => Ok(HostEapolAction::Inspect { proof }),
        }
    }

    fn handle_m1(
        &mut self,
        station_mac: [u8; ETHER_ADDR_LEN],
        packet: &[u8],
        proof: HostEapolFrameProof,
        tx_frame: &mut [u8; MAX_FRAME_LEN],
    ) -> Result<HostEapolAction, &'static str> {
        if !proof.pairwise || !proof.ack || proof.mic || proof.install || proof.secure {
            return Err("host-eapol-m1-shape");
        }
        let body = host_eapol_key_body(packet).ok_or("host-eapol-m1-body")?;
        let ap_mac = ethernet_src(packet).ok_or("host-eapol-m1-src")?;
        let mut anonce = [0u8; WPA_NONCE_LEN];
        anonce.copy_from_slice(
            &body[EAPOL_KEY_BODY_NONCE_OFFSET..EAPOL_KEY_BODY_NONCE_OFFSET + WPA_NONCE_LEN],
        );
        let mut replay_counter = [0u8; WPA_REPLAY_COUNTER_LEN];
        replay_counter.copy_from_slice(
            &body[EAPOL_KEY_BODY_REPLAY_OFFSET
                ..EAPOL_KEY_BODY_REPLAY_OFFSET + WPA_REPLAY_COUNTER_LEN],
        );
        self.ap_mac = ap_mac;
        self.anonce = anonce;
        self.m1_replay_counter = replay_counter;
        self.snonce = derive_host_snonce(
            &self.pmk,
            &self.ap_mac,
            &station_mac,
            &self.anonce,
            self.rx_packets,
        );
        self.ptk = derive_wpa2_pairwise_ptk(
            &self.pmk,
            &self.ap_mac,
            &station_mac,
            &self.anonce,
            &self.snonce,
        );
        self.record_current_ptk_candidate();
        let len = write_eapol_key_reply_frame(
            tx_frame,
            &self.ap_mac,
            &station_mac,
            EAPOL_KEY_INFO_M2,
            &self.m1_replay_counter,
            Some(&self.snonce),
            &WPA2_PSK_CCMP_RSN_IE,
            &self.ptk[..WPA_KCK_LEN],
        )?;
        self.m2_sent = true;
        Ok(HostEapolAction::SendM2 { len })
    }

    fn handle_m3(
        &mut self,
        station_mac: [u8; ETHER_ADDR_LEN],
        packet: &[u8],
        proof: HostEapolFrameProof,
        tx_frame: &mut [u8; MAX_FRAME_LEN],
    ) -> Result<HostEapolAction, &'static str> {
        if !self.m2_sent {
            return Err("host-eapol-m3-before-m2");
        }
        if proof.key_info & EAPOL_KEY_INFO_PAIRWISE_RECV_MASK != EAPOL_KEY_INFO_PAIRWISE_RECV_MASK
            || !proof.secure
            || !proof.encrypted_key_data
        {
            return Err("host-eapol-m3-shape");
        }
        if proof.key_len != 0 && proof.key_len != WPA_TK_LEN as u16 {
            return Err("host-eapol-m3-key-len");
        }
        let body = host_eapol_key_body(packet).ok_or("host-eapol-m3-body")?;
        let anonce =
            &body[EAPOL_KEY_BODY_NONCE_OFFSET..EAPOL_KEY_BODY_NONCE_OFFSET + WPA_NONCE_LEN];
        let ap_mac = ethernet_src(packet).ok_or("host-eapol-m3-src")?;
        if !self.ptk_candidate_matches_m3_identity(ap_mac, anonce) {
            return Err("host-eapol-m3-anonce");
        }
        let replay_counter = &body
            [EAPOL_KEY_BODY_REPLAY_OFFSET..EAPOL_KEY_BODY_REPLAY_OFFSET + WPA_REPLAY_COUNTER_LEN];
        let Some(matched_candidate) =
            self.select_m3_ptk_candidate(packet, ap_mac, anonce, replay_counter)?
        else {
            if !self.ptk_candidate_has_replay_window(ap_mac, anonce, replay_counter) {
                return Err("host-eapol-m3-replay");
            }
            return Err("host-eapol-m3-mic");
        };
        self.apply_ptk_candidate(matched_candidate);
        self.m3_replay_counter.copy_from_slice(replay_counter);
        let key_data = host_eapol_key_data(body).ok_or("host-eapol-m3-key-data")?;
        let mut unwrapped = [0u8; HOST_EAPOL_KEY_DATA_MAX_LEN];
        let unwrapped_len = aes128_key_unwrap(
            &self.ptk[WPA_KCK_LEN..WPA_KCK_LEN + WPA_KEK_LEN],
            key_data,
            &mut unwrapped,
        )?;
        if !eapol_key_data_contains_compatible_rsn_ie(&unwrapped[..unwrapped_len]) {
            return Err("host-eapol-m3-rsn-ie");
        }
        let gtk = match find_gtk_kde(&unwrapped[..unwrapped_len]) {
            Ok(gtk) => Some(gtk),
            Err("eapol-gtk-kde-missing") => None,
            Err(err) => return Err(err),
        };
        let mut rsc = [0u8; 6];
        rsc.copy_from_slice(&body[EAPOL_KEY_BODY_RSC_OFFSET..EAPOL_KEY_BODY_RSC_OFFSET + 6]);
        let len = write_eapol_key_reply_frame(
            tx_frame,
            &self.ap_mac,
            &station_mac,
            EAPOL_KEY_INFO_M4,
            &self.m3_replay_counter,
            None,
            &[],
            &self.ptk[..WPA_KCK_LEN],
        )?;
        let mut pairwise_tk = [0u8; WPA_TK_LEN];
        pairwise_tk.copy_from_slice(
            &self.ptk[WPA_KCK_LEN + WPA_KEK_LEN..WPA_KCK_LEN + WPA_KEK_LEN + WPA_TK_LEN],
        );
        self.m4_sent = true;
        self.group_replay_counter_valid = false;
        self.ptk_installed = true;
        self.gtk_installed = gtk.is_some();
        Ok(HostEapolAction::SendM4InstallKeys {
            len,
            keys: HostEapolInstallKeys {
                ap_mac: self.ap_mac,
                pairwise_tk,
                gtk,
                rsc,
            },
        })
    }

    fn record_current_ptk_candidate(&mut self) {
        let candidate = HostPtkCandidate {
            valid: true,
            ap_mac: self.ap_mac,
            anonce: self.anonce,
            snonce: self.snonce,
            m1_replay_counter: self.m1_replay_counter,
            ptk: self.ptk,
        };
        if let Some(slot) = self.ptk_candidates.iter_mut().find(|slot| {
            slot.valid
                && slot.ap_mac == candidate.ap_mac
                && slot.anonce == candidate.anonce
                && slot.m1_replay_counter == candidate.m1_replay_counter
                && slot.snonce == candidate.snonce
        }) {
            *slot = candidate;
            return;
        }
        self.ptk_candidates[self.ptk_candidate_next] = candidate;
        self.ptk_candidate_next = (self.ptk_candidate_next + 1) % HOST_EAPOL_PTK_CANDIDATES;
        self.ptk_candidate_count =
            core::cmp::min(self.ptk_candidate_count + 1, HOST_EAPOL_PTK_CANDIDATES);
    }

    fn ptk_candidate_matches_m3_identity(
        &self,
        ap_mac: [u8; ETHER_ADDR_LEN],
        anonce: &[u8],
    ) -> bool {
        self.ptk_candidates.iter().any(|candidate| {
            candidate.valid && candidate.ap_mac == ap_mac && candidate.anonce.as_slice() == anonce
        })
    }

    fn ptk_candidate_has_replay_window(
        &self,
        ap_mac: [u8; ETHER_ADDR_LEN],
        anonce: &[u8],
        replay_counter: &[u8],
    ) -> bool {
        self.ptk_candidates.iter().any(|candidate| {
            candidate.valid
                && candidate.ap_mac == ap_mac
                && candidate.anonce.as_slice() == anonce
                && replay_counter_increases(replay_counter, &candidate.m1_replay_counter)
        })
    }

    fn select_m3_ptk_candidate(
        &self,
        packet: &[u8],
        ap_mac: [u8; ETHER_ADDR_LEN],
        anonce: &[u8],
        replay_counter: &[u8],
    ) -> Result<Option<HostPtkCandidate>, &'static str> {
        for candidate in self.ptk_candidates {
            if !candidate.valid
                || candidate.ap_mac != ap_mac
                || candidate.anonce.as_slice() != anonce
                || !replay_counter_increases(replay_counter, &candidate.m1_replay_counter)
            {
                continue;
            }
            if verify_eapol_key_mic(packet, &candidate.ptk[..WPA_KCK_LEN])? {
                return Ok(Some(candidate));
            }
        }
        Ok(None)
    }

    fn apply_ptk_candidate(&mut self, candidate: HostPtkCandidate) {
        self.ap_mac = candidate.ap_mac;
        self.anonce = candidate.anonce;
        self.snonce = candidate.snonce;
        self.m1_replay_counter = candidate.m1_replay_counter;
        self.ptk = candidate.ptk;
    }

    fn handle_group_key(
        &mut self,
        station_mac: [u8; ETHER_ADDR_LEN],
        packet: &[u8],
        proof: HostEapolFrameProof,
        tx_frame: &mut [u8; MAX_FRAME_LEN],
    ) -> Result<HostEapolAction, &'static str> {
        if !self.m4_sent {
            return Err("host-eapol-group-before-m4");
        }
        if proof.pairwise || !proof.mic || !proof.secure || !proof.encrypted_key_data {
            return Err("host-eapol-group-shape");
        }
        let body = host_eapol_key_body(packet).ok_or("host-eapol-group-body")?;
        let replay_counter = &body
            [EAPOL_KEY_BODY_REPLAY_OFFSET..EAPOL_KEY_BODY_REPLAY_OFFSET + WPA_REPLAY_COUNTER_LEN];
        if !group_replay_counter_admitted(replay_counter, self) {
            return Err("host-eapol-group-replay");
        }
        if !verify_eapol_key_mic(packet, &self.ptk[..WPA_KCK_LEN])? {
            return Err("host-eapol-group-mic");
        }
        let key_data = host_eapol_key_data(body).ok_or("host-eapol-group-key-data")?;
        let mut unwrapped = [0u8; HOST_EAPOL_KEY_DATA_MAX_LEN];
        let unwrapped_len = aes128_key_unwrap(
            &self.ptk[WPA_KCK_LEN..WPA_KCK_LEN + WPA_KEK_LEN],
            key_data,
            &mut unwrapped,
        )?;
        let gtk = find_gtk_kde(&unwrapped[..unwrapped_len])?;
        let mut rsc = [0u8; 6];
        rsc.copy_from_slice(&body[EAPOL_KEY_BODY_RSC_OFFSET..EAPOL_KEY_BODY_RSC_OFFSET + 6]);
        let mut replay = [0u8; WPA_REPLAY_COUNTER_LEN];
        replay.copy_from_slice(replay_counter);
        let len = write_eapol_key_reply_frame(
            tx_frame,
            &self.ap_mac,
            &station_mac,
            EAPOL_KEY_INFO_GROUP_M2,
            &replay,
            None,
            &[],
            &self.ptk[..WPA_KCK_LEN],
        )?;
        self.group_replay_counter.copy_from_slice(replay_counter);
        self.group_replay_counter_valid = true;
        self.gtk_installed = true;
        Ok(HostEapolAction::SendGroupM2InstallGtk {
            len,
            keys: HostEapolGroupKeys { gtk, rsc },
        })
    }
}

pub fn write_wsec_key_payload(
    payload: &mut [u8],
    index: u32,
    key: &[u8],
    ea: &[u8; ETHER_ADDR_LEN],
    rsc: Option<&[u8]>,
    primary: bool,
) -> Result<usize, &'static str> {
    if payload.len() < WSEC_KEY_PAYLOAD_LEN || key.len() > WSEC_KEY_DATA_LEN {
        return Err("wsec-key-payload-len");
    }
    payload[..WSEC_KEY_PAYLOAD_LEN].fill(0);
    put_u32_le(payload, WSEC_KEY_INDEX_OFFSET, index);
    put_u32_le(payload, WSEC_KEY_LEN_OFFSET, key.len() as u32);
    payload[WSEC_KEY_DATA_OFFSET..WSEC_KEY_DATA_OFFSET + key.len()].copy_from_slice(key);
    put_u32_le(payload, WSEC_KEY_ALGO_OFFSET, CRYPTO_ALGO_AES_CCM);
    put_u32_le(
        payload,
        WSEC_KEY_FLAGS_OFFSET,
        if primary { BRCMF_PRIMARY_KEY } else { 0 },
    );
    if let Some(rsc) = rsc {
        if rsc.len() >= 6 {
            put_u32_le(payload, WSEC_KEY_IV_INITIALIZED_OFFSET, 1);
            put_u32_le(
                payload,
                WSEC_KEY_RXIV_HI_OFFSET,
                u32::from(rsc[2])
                    | (u32::from(rsc[3]) << 8)
                    | (u32::from(rsc[4]) << 16)
                    | (u32::from(rsc[5]) << 24),
            );
            put_u16_le(
                payload,
                WSEC_KEY_RXIV_LO_OFFSET,
                u16::from(rsc[0]) | (u16::from(rsc[1]) << 8),
            );
        }
    }
    payload[WSEC_KEY_EA_OFFSET..WSEC_KEY_EA_OFFSET + ea.len()].copy_from_slice(ea);
    Ok(WSEC_KEY_PAYLOAD_LEN)
}

pub fn write_eapol_start_frame(
    frame: &mut [u8],
    dst: &[u8; ETHER_ADDR_LEN],
    src: &[u8; ETHER_ADDR_LEN],
) -> Result<usize, &'static str> {
    let len = ETH_HEADER_LEN
        .checked_add(EAPOL_HEADER_LEN)
        .ok_or("eapol-start-frame-len")?;
    if frame.len() < len {
        return Err("eapol-start-frame-len");
    }
    frame[..len].fill(0);
    frame[..ETHER_ADDR_LEN].copy_from_slice(dst);
    frame[ETHER_ADDR_LEN..ETHER_ADDR_LEN * 2].copy_from_slice(src);
    put_u16_be(frame, 12, ETH_P_EAPOL);
    frame[ETH_HEADER_LEN] = EAPOL_VERSION_8021X_2004;
    frame[ETH_HEADER_LEN + 1] = EAPOL_PACKET_TYPE_START;
    put_u16_be(frame, ETH_HEADER_LEN + 2, 0);
    Ok(len)
}

fn fill_wpa2_psk_pmk(
    ssid: &[u8],
    psk: &[u8],
    output: &mut [u8; WSEC_PMK_LEN],
) -> Result<(), &'static str> {
    if psk.len() == WPA2_PSK_HEX_PMK_LEN {
        return decode_hex_pmk(psk, output);
    }
    if psk.len() < WPA2_PSK_MIN_PASSPHRASE_LEN || psk.len() > WPA2_PSK_MAX_PASSPHRASE_LEN {
        return Err("wifi-psk-invalid");
    }
    for block_index in 1..=WPA2_PSK_BLOCK_COUNT {
        let block_suffix = block_index.to_be_bytes();
        let mut u = hmac_sha1(psk, ssid, &block_suffix);
        let mut t = u;
        for _ in 1..WPA2_PSK_PBKDF2_ROUNDS {
            u = hmac_sha1(psk, &u, &[]);
            for index in 0..SHA1_DIGEST_LEN {
                t[index] ^= u[index];
            }
        }
        let output_offset = (block_index as usize - 1) * SHA1_DIGEST_LEN;
        let copy_len = core::cmp::min(SHA1_DIGEST_LEN, WSEC_PMK_LEN - output_offset);
        output[output_offset..output_offset + copy_len].copy_from_slice(&t[..copy_len]);
    }
    Ok(())
}

fn decode_hex_pmk(psk: &[u8], output: &mut [u8; WSEC_PMK_LEN]) -> Result<(), &'static str> {
    if psk.len() != WPA2_PSK_HEX_PMK_LEN {
        return Err("wifi-psk-invalid");
    }
    for (index, pair) in psk.chunks_exact(2).enumerate() {
        let hi = hex_nibble(pair[0]).ok_or("wifi-psk-invalid")?;
        let lo = hex_nibble(pair[1]).ok_or("wifi-psk-invalid")?;
        output[index] = (hi << 4) | lo;
    }
    Ok(())
}

const fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn hmac_sha1(key: &[u8], first: &[u8], second: &[u8]) -> [u8; SHA1_DIGEST_LEN] {
    hmac_sha1_three(key, first, second, &[])
}

fn hmac_sha1_three(key: &[u8], first: &[u8], second: &[u8], third: &[u8]) -> [u8; SHA1_DIGEST_LEN] {
    let mut key_block = [0u8; SHA1_BLOCK_LEN];
    if key.len() > SHA1_BLOCK_LEN {
        let digest = sha1_digest(key, &[], &[]);
        key_block[..SHA1_DIGEST_LEN].copy_from_slice(&digest);
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }
    let mut inner_pad = [0x36u8; SHA1_BLOCK_LEN];
    let mut outer_pad = [0x5cu8; SHA1_BLOCK_LEN];
    for index in 0..SHA1_BLOCK_LEN {
        inner_pad[index] ^= key_block[index];
        outer_pad[index] ^= key_block[index];
    }
    let mut inner_state = Sha1State::new();
    inner_state.update(&inner_pad);
    inner_state.update(first);
    inner_state.update(second);
    inner_state.update(third);
    let inner = inner_state.finalize();
    sha1_digest(&outer_pad, &inner, &[])
}

fn sha1_digest(first: &[u8], second: &[u8], third: &[u8]) -> [u8; SHA1_DIGEST_LEN] {
    let mut state = Sha1State::new();
    state.update(first);
    state.update(second);
    state.update(third);
    state.finalize()
}

#[derive(Clone)]
struct Sha1State {
    state: [u32; 5],
    buffer: [u8; SHA1_BLOCK_LEN],
    buffer_len: usize,
    total_len: u64,
}

impl Sha1State {
    const fn new() -> Self {
        Self {
            state: [
                0x6745_2301,
                0xefcd_ab89,
                0x98ba_dcfe,
                0x1032_5476,
                0xc3d2_e1f0,
            ],
            buffer: [0; SHA1_BLOCK_LEN],
            buffer_len: 0,
            total_len: 0,
        }
    }

    fn update(&mut self, mut input: &[u8]) {
        self.total_len = self.total_len.wrapping_add(input.len() as u64);
        if self.buffer_len != 0 {
            let fill = core::cmp::min(SHA1_BLOCK_LEN - self.buffer_len, input.len());
            self.buffer[self.buffer_len..self.buffer_len + fill].copy_from_slice(&input[..fill]);
            self.buffer_len += fill;
            input = &input[fill..];
            if self.buffer_len == SHA1_BLOCK_LEN {
                let block = self.buffer;
                self.process_block(&block);
                self.buffer_len = 0;
            }
        }
        while input.len() >= SHA1_BLOCK_LEN {
            let mut block = [0u8; SHA1_BLOCK_LEN];
            block.copy_from_slice(&input[..SHA1_BLOCK_LEN]);
            self.process_block(&block);
            input = &input[SHA1_BLOCK_LEN..];
        }
        if !input.is_empty() {
            self.buffer[..input.len()].copy_from_slice(input);
            self.buffer_len = input.len();
        }
    }

    fn finalize(mut self) -> [u8; SHA1_DIGEST_LEN] {
        let bit_len = self.total_len.wrapping_mul(8);
        self.update(&[0x80]);
        let zeros = [0u8; SHA1_BLOCK_LEN];
        while self.buffer_len != 56 {
            let fill = if self.buffer_len < 56 {
                56 - self.buffer_len
            } else {
                SHA1_BLOCK_LEN - self.buffer_len
            };
            self.update(&zeros[..fill]);
        }
        self.update(&bit_len.to_be_bytes());
        let mut digest = [0u8; SHA1_DIGEST_LEN];
        for (index, word) in self.state.iter().copied().enumerate() {
            digest[index * 4..index * 4 + 4].copy_from_slice(&word.to_be_bytes());
        }
        digest
    }

    fn process_block(&mut self, block: &[u8; SHA1_BLOCK_LEN]) {
        let mut schedule = [0u32; 80];
        for (index, chunk) in block.chunks_exact(4).enumerate().take(16) {
            schedule[index] = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }
        for index in 16..80 {
            schedule[index] = (schedule[index - 3]
                ^ schedule[index - 8]
                ^ schedule[index - 14]
                ^ schedule[index - 16])
                .rotate_left(1);
        }
        let mut a = self.state[0];
        let mut b = self.state[1];
        let mut c = self.state[2];
        let mut d = self.state[3];
        let mut e = self.state[4];
        for (index, word) in schedule.iter().copied().enumerate() {
            let (f, k) = match index {
                0..=19 => ((b & c) | ((!b) & d), 0x5a82_7999),
                20..=39 => (b ^ c ^ d, 0x6ed9_eba1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8f1b_bcdc),
                _ => (b ^ c ^ d, 0xca62_c1d6),
            };
            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }
        self.state[0] = self.state[0].wrapping_add(a);
        self.state[1] = self.state[1].wrapping_add(b);
        self.state[2] = self.state[2].wrapping_add(c);
        self.state[3] = self.state[3].wrapping_add(d);
        self.state[4] = self.state[4].wrapping_add(e);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostEapolFrameProof {
    pub key_info: u16,
    pub key_len: u16,
    pub key_version: u16,
    pub key_data_len: u16,
    pub pairwise: bool,
    pub ack: bool,
    pub mic: bool,
    pub install: bool,
    pub secure: bool,
    pub encrypted_key_data: bool,
    pub nonce_present: bool,
    pub replay_counter_nonzero: bool,
    pub message: &'static str,
    pub next_action: &'static str,
}

impl HostEapolFrameProof {
    fn parse(packet: &[u8]) -> Result<Self, &'static str> {
        if packet.len() < ETH_HEADER_LEN + EAPOL_HEADER_LEN + EAPOL_KEY_MIN_BODY_LEN {
            return Err("host-eapol-short");
        }
        let eapol = &packet[ETH_HEADER_LEN..];
        if eapol[0] != EAPOL_VERSION_8021X_2004 || eapol[1] != EAPOL_PACKET_TYPE_KEY {
            return Err("host-eapol-non-key");
        }
        let body_len = usize::from(get_u16_be(eapol, 2).ok_or("host-eapol-body-len")?);
        if body_len < EAPOL_KEY_MIN_BODY_LEN
            || ETH_HEADER_LEN + EAPOL_HEADER_LEN + body_len > packet.len()
        {
            return Err("host-eapol-body-shape");
        }
        let body = &eapol[EAPOL_HEADER_LEN..EAPOL_HEADER_LEN + body_len];
        if body[EAPOL_KEY_BODY_DESCRIPTOR_OFFSET] != EAPOL_KEY_DESCRIPTOR_RSN {
            return Err("host-eapol-descriptor");
        }
        let key_info =
            get_u16_be(body, EAPOL_KEY_BODY_KEY_INFO_OFFSET).ok_or("host-eapol-key-info")?;
        let pairwise = key_info & EAPOL_KEY_INFO_KEY_TYPE != 0;
        let ack = key_info & EAPOL_KEY_INFO_ACK != 0;
        let mic = key_info & EAPOL_KEY_INFO_MIC != 0;
        let install = key_info & EAPOL_KEY_INFO_INSTALL != 0;
        let secure = key_info & EAPOL_KEY_INFO_SECURE != 0;
        let encrypted_key_data = key_info & EAPOL_KEY_INFO_ENCRYPTED_KEY_DATA != 0;
        let nonce_present = body
            [EAPOL_KEY_BODY_NONCE_OFFSET..EAPOL_KEY_BODY_NONCE_OFFSET + WPA_NONCE_LEN]
            .iter()
            .any(|byte| *byte != 0);
        let replay_counter_nonzero = body
            [EAPOL_KEY_BODY_REPLAY_OFFSET..EAPOL_KEY_BODY_REPLAY_OFFSET + WPA_REPLAY_COUNTER_LEN]
            .iter()
            .any(|byte| *byte != 0);
        let message = classify_eapol_key_message(key_info);
        Ok(Self {
            key_info,
            key_len: get_u16_be(body, EAPOL_KEY_BODY_KEY_LEN_OFFSET).ok_or("host-eapol-key-len")?,
            key_version: key_info & EAPOL_KEY_INFO_KEY_VERSION_MASK,
            key_data_len: get_u16_be(body, EAPOL_KEY_BODY_DATA_LEN_OFFSET)
                .ok_or("host-eapol-key-data-len")?,
            pairwise,
            ack,
            mic,
            install,
            secure,
            encrypted_key_data,
            nonce_present,
            replay_counter_nonzero,
            message,
            next_action: host_eapol_next_action(message),
        })
    }
}

pub fn inspect_host_eapol_frame(packet: &[u8]) -> Option<HostEapolFrameProof> {
    if ethernet_ethertype(packet) == Some(ETH_P_EAPOL) {
        HostEapolFrameProof::parse(packet).ok()
    } else {
        None
    }
}

const fn classify_eapol_key_message(key_info: u16) -> &'static str {
    let pairwise = key_info & EAPOL_KEY_INFO_KEY_TYPE != 0;
    let ack = key_info & EAPOL_KEY_INFO_ACK != 0;
    let mic = key_info & EAPOL_KEY_INFO_MIC != 0;
    let install = key_info & EAPOL_KEY_INFO_INSTALL != 0;
    let secure = key_info & EAPOL_KEY_INFO_SECURE != 0;
    let encrypted = key_info & EAPOL_KEY_INFO_ENCRYPTED_KEY_DATA != 0;
    if pairwise && ack && !mic {
        "m1"
    } else if pairwise && !ack && mic && !secure {
        "m2"
    } else if pairwise && ack && mic && install && encrypted {
        "m3"
    } else if pairwise && !ack && mic && secure {
        "m4"
    } else if !pairwise && mic {
        "group-key"
    } else {
        "unknown"
    }
}

fn host_eapol_next_action(message: &str) -> &'static str {
    match message.as_bytes() {
        b"m1" => "derive-ptk-send-m2",
        b"m3" => "verify-mic-send-m4-install-keys",
        b"group-key" => "verify-mic-install-gtk",
        b"m2" | b"m4" => "unexpected-sta-message",
        _ => "inspect-host-eapol",
    }
}

fn host_eapol_key_body(packet: &[u8]) -> Option<&[u8]> {
    let eapol = packet.get(ETH_HEADER_LEN..)?;
    let body_len = usize::from(get_u16_be(eapol, 2)?);
    eapol.get(EAPOL_HEADER_LEN..EAPOL_HEADER_LEN + body_len)
}

fn host_eapol_key_data(body: &[u8]) -> Option<&[u8]> {
    let key_data_len = usize::from(get_u16_be(body, EAPOL_KEY_BODY_DATA_LEN_OFFSET)?);
    body.get(EAPOL_KEY_BODY_DATA_OFFSET..EAPOL_KEY_BODY_DATA_OFFSET + key_data_len)
}

fn write_eapol_key_reply_frame(
    frame: &mut [u8; MAX_FRAME_LEN],
    dst: &[u8; ETHER_ADDR_LEN],
    src: &[u8; ETHER_ADDR_LEN],
    key_info: u16,
    replay_counter: &[u8; WPA_REPLAY_COUNTER_LEN],
    nonce: Option<&[u8; WPA_NONCE_LEN]>,
    key_data: &[u8],
    kck: &[u8],
) -> Result<usize, &'static str> {
    let body_len = EAPOL_KEY_MIN_BODY_LEN
        .checked_add(key_data.len())
        .ok_or("eapol-frame-len")?;
    let len = ETH_HEADER_LEN
        .checked_add(EAPOL_HEADER_LEN)
        .and_then(|value| value.checked_add(body_len))
        .ok_or("eapol-frame-len")?;
    if len > frame.len() || body_len > u16::MAX as usize || key_data.len() > u16::MAX as usize {
        return Err("eapol-frame-len");
    }
    frame[..len].fill(0);
    frame[..6].copy_from_slice(dst);
    frame[6..12].copy_from_slice(src);
    put_u16_be(frame, 12, ETH_P_EAPOL);
    frame[ETH_HEADER_LEN] = EAPOL_VERSION_8021X_2004;
    frame[ETH_HEADER_LEN + 1] = EAPOL_PACKET_TYPE_KEY;
    put_u16_be(frame, ETH_HEADER_LEN + 2, body_len as u16);
    let body = ETH_HEADER_LEN + EAPOL_HEADER_LEN;
    frame[body + EAPOL_KEY_BODY_DESCRIPTOR_OFFSET] = EAPOL_KEY_DESCRIPTOR_RSN;
    put_u16_be(frame, body + EAPOL_KEY_BODY_KEY_INFO_OFFSET, key_info);
    put_u16_be(
        frame,
        body + EAPOL_KEY_BODY_KEY_LEN_OFFSET,
        WPA_EAPOL_REPLY_KEY_LEN,
    );
    frame[body + EAPOL_KEY_BODY_REPLAY_OFFSET
        ..body + EAPOL_KEY_BODY_REPLAY_OFFSET + WPA_REPLAY_COUNTER_LEN]
        .copy_from_slice(replay_counter);
    if let Some(nonce) = nonce {
        frame[body + EAPOL_KEY_BODY_NONCE_OFFSET
            ..body + EAPOL_KEY_BODY_NONCE_OFFSET + WPA_NONCE_LEN]
            .copy_from_slice(nonce);
    }
    put_u16_be(
        frame,
        body + EAPOL_KEY_BODY_DATA_LEN_OFFSET,
        key_data.len() as u16,
    );
    frame[body + EAPOL_KEY_BODY_DATA_OFFSET..body + EAPOL_KEY_BODY_DATA_OFFSET + key_data.len()]
        .copy_from_slice(key_data);
    let mic = hmac_sha1(kck, &frame[ETH_HEADER_LEN..len], &[]);
    frame[body + EAPOL_KEY_BODY_MIC_OFFSET..body + EAPOL_KEY_BODY_MIC_OFFSET + WPA_MIC_LEN]
        .copy_from_slice(&mic[..WPA_MIC_LEN]);
    Ok(len)
}

fn verify_eapol_key_mic(packet: &[u8], kck: &[u8]) -> Result<bool, &'static str> {
    let canonical_len = host_eapol_canonical_len(packet)?;
    let body_offset = ETH_HEADER_LEN + EAPOL_HEADER_LEN;
    let mic_offset = body_offset + EAPOL_KEY_BODY_MIC_OFFSET;
    let mic_end = mic_offset + WPA_MIC_LEN;
    let expected = packet.get(mic_offset..mic_end).ok_or("eapol-mic-missing")?;
    let mut copy = [0u8; MAX_FRAME_LEN];
    copy[..canonical_len].copy_from_slice(&packet[..canonical_len]);
    copy[mic_offset..mic_end].fill(0);
    let actual = hmac_sha1(kck, &copy[ETH_HEADER_LEN..canonical_len], &[]);
    Ok(constant_time_eq(expected, &actual[..WPA_MIC_LEN]))
}

fn host_eapol_canonical_len(packet: &[u8]) -> Result<usize, &'static str> {
    let eapol = packet.get(ETH_HEADER_LEN..).ok_or("eapol-short-ethernet")?;
    if eapol.len() < EAPOL_HEADER_LEN {
        return Err("eapol-short-header");
    }
    let body_len = usize::from(get_u16_be(eapol, 2).ok_or("eapol-body-len")?);
    let canonical_len = ETH_HEADER_LEN
        .checked_add(EAPOL_HEADER_LEN)
        .and_then(|value| value.checked_add(body_len))
        .ok_or("eapol-body-len")?;
    if canonical_len > packet.len() {
        return Err("eapol-body-truncated");
    }
    Ok(canonical_len)
}

fn aes128_key_unwrap(
    kek: &[u8],
    wrapped: &[u8],
    output: &mut [u8; HOST_EAPOL_KEY_DATA_MAX_LEN],
) -> Result<usize, &'static str> {
    if kek.len() != WPA_KEK_LEN || wrapped.len() < 16 || wrapped.len() % 8 != 0 {
        return Err("eapol-key-data-wrap-shape");
    }
    let n = wrapped.len() / 8 - 1;
    let plain_len = n * 8;
    if plain_len > output.len() {
        return Err("eapol-key-data-too-large");
    }
    let mut a = [0u8; 8];
    a.copy_from_slice(&wrapped[..8]);
    output[..plain_len].copy_from_slice(&wrapped[8..]);
    let cipher = Aes128::new(GenericArray::from_slice(kek));
    let mut j = 6usize;
    while j > 0 {
        j -= 1;
        let mut i = n;
        while i > 0 {
            let t = (n * j + i) as u64;
            let mut block = [0u8; 16];
            block[..8].copy_from_slice(&a);
            xor_key_wrap_t(&mut block[..8], t);
            block[8..].copy_from_slice(&output[(i - 1) * 8..i * 8]);
            cipher.decrypt_block(GenericArray::from_mut_slice(&mut block));
            a.copy_from_slice(&block[..8]);
            output[(i - 1) * 8..i * 8].copy_from_slice(&block[8..]);
            i -= 1;
        }
    }
    if a != [0xa6; 8] {
        return Err("eapol-key-data-wrap-integrity");
    }
    Ok(plain_len)
}

fn find_gtk_kde(key_data: &[u8]) -> Result<HostGtkKey, &'static str> {
    let mut offset = 0usize;
    while offset + 2 <= key_data.len() {
        let element_id = key_data[offset];
        let element_len = usize::from(key_data[offset + 1]);
        if element_id == 0 && key_data[offset..].iter().all(|byte| *byte == 0) {
            break;
        }
        if element_id == 0xdd && eapol_key_data_padding_tail(&key_data[offset..]) {
            break;
        }
        let element_start = offset + 2;
        let element_end = element_start
            .checked_add(element_len)
            .ok_or("eapol-gtk-len")?;
        if element_end > key_data.len() {
            return Err("eapol-gtk-kde-truncated");
        }
        let element = &key_data[element_start..element_end];
        if element_id == 0xdd
            && element_len >= 6
            && element.get(..3) == Some(RSN_KDE_OUI.as_slice())
            && element.get(3).copied() == Some(RSN_KDE_TYPE_GTK)
        {
            let key_info = element[4];
            let gtk = element.get(6..).ok_or("eapol-gtk-kde-short")?;
            if gtk.len() > WSEC_KEY_DATA_LEN || gtk.len() < WPA_TK_LEN {
                return Err("eapol-gtk-len");
            }
            let mut key = [0u8; WSEC_KEY_DATA_LEN];
            key[..gtk.len()].copy_from_slice(gtk);
            return Ok(HostGtkKey {
                index: key_info & 0x03,
                key_len: gtk.len(),
                key,
            });
        }
        offset = element_end;
    }
    Err("eapol-gtk-kde-missing")
}

fn eapol_key_data_contains_compatible_rsn_ie(key_data: &[u8]) -> bool {
    let mut offset = 0usize;
    while offset + 2 <= key_data.len() {
        let element_id = key_data[offset];
        let element_len = usize::from(key_data[offset + 1]);
        if element_id == 0 && key_data[offset..].iter().all(|byte| *byte == 0) {
            break;
        }
        if element_id == 0xdd && eapol_key_data_padding_tail(&key_data[offset..]) {
            break;
        }
        let Some(element_end) = offset.checked_add(2 + element_len) else {
            return false;
        };
        if element_end > key_data.len() {
            return false;
        }
        if element_id == RSN_IE_ID
            && rsn_ie_is_wpa2_psk_ccmp_compatible(&key_data[offset..element_end])
        {
            return true;
        }
        offset = element_end;
    }
    false
}

fn rsn_ie_is_wpa2_psk_ccmp_compatible(ie: &[u8]) -> bool {
    if ie.len() < 2 || ie[0] != RSN_IE_ID || usize::from(ie[1]) + 2 != ie.len() {
        return false;
    }
    let mut offset = 2usize;
    if get_u16_le(ie, offset) != Some(RSN_VERSION_1) {
        return false;
    }
    offset += 2;
    if offset + RSN_SUITE_LEN > ie.len()
        || !rsn_suite_matches(&ie[offset..offset + RSN_SUITE_LEN], RSN_CIPHER_CCMP)
    {
        return false;
    }
    offset += RSN_SUITE_LEN;
    let Some(pairwise_count) = get_u16_le(ie, offset).map(usize::from) else {
        return false;
    };
    offset += 2;
    let mut pairwise_ccmp = false;
    for _ in 0..pairwise_count {
        if offset + RSN_SUITE_LEN > ie.len() {
            return false;
        }
        pairwise_ccmp |= rsn_suite_matches(&ie[offset..offset + RSN_SUITE_LEN], RSN_CIPHER_CCMP);
        offset += RSN_SUITE_LEN;
    }
    let Some(akm_count) = get_u16_le(ie, offset).map(usize::from) else {
        return false;
    };
    offset += 2;
    let mut akm_psk = false;
    for _ in 0..akm_count {
        if offset + RSN_SUITE_LEN > ie.len() {
            return false;
        }
        akm_psk |= rsn_suite_matches(&ie[offset..offset + RSN_SUITE_LEN], RSN_AKM_PSK);
        offset += RSN_SUITE_LEN;
    }
    pairwise_ccmp && akm_psk
}

fn rsn_suite_matches(slice: &[u8], suite_type: u8) -> bool {
    slice.len() == RSN_SUITE_LEN
        && slice.get(..3) == Some(RSN_KDE_OUI.as_slice())
        && slice.get(3).copied() == Some(suite_type)
}

fn eapol_key_data_padding_tail(tail: &[u8]) -> bool {
    !tail.is_empty() && tail.iter().all(|byte| *byte == 0 || *byte == 0xdd)
}

fn derive_host_snonce(
    pmk: &[u8; WSEC_PMK_LEN],
    ap_mac: &[u8; ETHER_ADDR_LEN],
    sta_mac: &[u8; ETHER_ADDR_LEN],
    anonce: &[u8; WPA_NONCE_LEN],
    rx_count: u32,
) -> [u8; WPA_NONCE_LEN] {
    let mut seed = [0u8; ETHER_ADDR_LEN + ETHER_ADDR_LEN + WPA_NONCE_LEN + 4];
    seed[..ETHER_ADDR_LEN].copy_from_slice(ap_mac);
    seed[ETHER_ADDR_LEN..ETHER_ADDR_LEN * 2].copy_from_slice(sta_mac);
    seed[ETHER_ADDR_LEN * 2..ETHER_ADDR_LEN * 2 + WPA_NONCE_LEN].copy_from_slice(anonce);
    seed[ETHER_ADDR_LEN * 2 + WPA_NONCE_LEN..].copy_from_slice(&rx_count.to_be_bytes());
    let first = hmac_sha1_three(pmk, WPA_SNONCE_LABEL_PREFIX, &seed, &[0]);
    let second = hmac_sha1_three(pmk, WPA_SNONCE_LABEL_PREFIX, &seed, &[1]);
    let mut snonce = [0u8; WPA_NONCE_LEN];
    snonce[..SHA1_DIGEST_LEN].copy_from_slice(&first);
    snonce[SHA1_DIGEST_LEN..].copy_from_slice(&second[..WPA_NONCE_LEN - SHA1_DIGEST_LEN]);
    snonce
}

fn derive_wpa2_pairwise_ptk(
    pmk: &[u8; WSEC_PMK_LEN],
    ap_mac: &[u8; ETHER_ADDR_LEN],
    sta_mac: &[u8; ETHER_ADDR_LEN],
    anonce: &[u8; WPA_NONCE_LEN],
    snonce: &[u8; WPA_NONCE_LEN],
) -> [u8; WPA_PTK_LEN] {
    let mut seed = [0u8; ETHER_ADDR_LEN + ETHER_ADDR_LEN + WPA_NONCE_LEN + WPA_NONCE_LEN];
    let (mac_low, mac_high) = if bytes_less(ap_mac, sta_mac) {
        (ap_mac, sta_mac)
    } else {
        (sta_mac, ap_mac)
    };
    seed[..ETHER_ADDR_LEN].copy_from_slice(mac_low);
    seed[ETHER_ADDR_LEN..ETHER_ADDR_LEN * 2].copy_from_slice(mac_high);
    let (nonce_low, nonce_high) = if bytes_less(anonce, snonce) {
        (anonce, snonce)
    } else {
        (snonce, anonce)
    };
    seed[ETHER_ADDR_LEN * 2..ETHER_ADDR_LEN * 2 + WPA_NONCE_LEN].copy_from_slice(nonce_low);
    seed[ETHER_ADDR_LEN * 2 + WPA_NONCE_LEN..].copy_from_slice(nonce_high);
    let mut ptk = [0u8; WPA_PTK_LEN];
    let mut output_offset = 0usize;
    let mut counter = 0u8;
    while output_offset < ptk.len() {
        let digest = hmac_sha1_three(pmk, WPA_PTK_PRF_LABEL, &seed, &[counter]);
        let copy_len = core::cmp::min(SHA1_DIGEST_LEN, ptk.len() - output_offset);
        ptk[output_offset..output_offset + copy_len].copy_from_slice(&digest[..copy_len]);
        output_offset += copy_len;
        counter = counter.wrapping_add(1);
    }
    ptk
}

fn packet_dst_allowed(packet: &[u8], station_mac: [u8; ETHER_ADDR_LEN]) -> bool {
    ethernet_dst(packet).is_some_and(|dst| {
        dst == station_mac || dst == PAE_GROUP_ADDR || mac_is_station_local_alias(dst, station_mac)
    })
}

fn mac_is_station_local_alias(
    mac: [u8; ETHER_ADDR_LEN],
    station_mac: [u8; ETHER_ADDR_LEN],
) -> bool {
    mac[1..] == station_mac[1..] && (mac[0] ^ station_mac[0]) == 0x02
}

fn replay_counter_increases(current: &[u8], previous: &[u8; WPA_REPLAY_COUNTER_LEN]) -> bool {
    if current.len() != previous.len() {
        return false;
    }
    for index in 0..current.len() {
        if current[index] != previous[index] {
            return current[index] > previous[index];
        }
    }
    false
}

fn group_replay_counter_admitted(current: &[u8], state: &HostEapolState) -> bool {
    let previous = if state.group_replay_counter_valid {
        &state.group_replay_counter
    } else {
        &state.m3_replay_counter
    };
    replay_counter_increases(current, previous)
        || (state.gtk_installed
            && state.group_replay_counter_valid
            && current == state.group_replay_counter.as_slice())
}

fn bytes_less<const N: usize>(left: &[u8; N], right: &[u8; N]) -> bool {
    for index in 0..N {
        if left[index] != right[index] {
            return left[index] < right[index];
        }
    }
    false
}

fn ethernet_ethertype(packet: &[u8]) -> Option<u16> {
    get_u16_be(packet, 12)
}

fn ethernet_src(packet: &[u8]) -> Option<[u8; ETHER_ADDR_LEN]> {
    let mut mac = [0u8; ETHER_ADDR_LEN];
    mac.copy_from_slice(packet.get(6..12)?);
    Some(mac)
}

fn ethernet_dst(packet: &[u8]) -> Option<[u8; ETHER_ADDR_LEN]> {
    let mut mac = [0u8; ETHER_ADDR_LEN];
    mac.copy_from_slice(packet.get(..6)?);
    Some(mac)
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0u8;
    for index in 0..left.len() {
        diff |= left[index] ^ right[index];
    }
    diff == 0
}

fn xor_key_wrap_t(slot: &mut [u8], t: u64) {
    let bytes = t.to_be_bytes();
    for index in 0..8.min(slot.len()) {
        slot[index] ^= bytes[index];
    }
}

fn get_u16_be(buf: &[u8], offset: usize) -> Option<u16> {
    let bytes = buf.get(offset..offset + 2)?;
    Some(u16::from_be_bytes([bytes[0], bytes[1]]))
}

fn get_u16_le(buf: &[u8], offset: usize) -> Option<u16> {
    let bytes = buf.get(offset..offset + 2)?;
    Some(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn put_u16_be(buf: &mut [u8], offset: usize, value: u16) {
    if let Some(slot) = buf.get_mut(offset..offset + 2) {
        slot.copy_from_slice(&value.to_be_bytes());
    }
}

fn put_u16_le(buf: &mut [u8], offset: usize, value: u16) {
    if let Some(slot) = buf.get_mut(offset..offset + 2) {
        slot.copy_from_slice(&value.to_le_bytes());
    }
}

fn put_u32_le(buf: &mut [u8], offset: usize, value: u32) {
    if let Some(slot) = buf.get_mut(offset..offset + 4) {
        slot.copy_from_slice(&value.to_le_bytes());
    }
}

#[cfg(test)]
fn test_aes128_key_wrap(
    kek: &[u8],
    plain: &[u8],
    output: &mut [u8; HOST_EAPOL_KEY_DATA_MAX_LEN],
) -> Result<usize, &'static str> {
    if kek.len() != WPA_KEK_LEN || plain.len() < 16 || plain.len() % 8 != 0 {
        return Err("eapol-key-data-test-wrap-shape");
    }
    let n = plain.len() / 8;
    let wrapped_len = plain
        .len()
        .checked_add(8)
        .ok_or("eapol-key-data-test-wrap-len")?;
    if wrapped_len > output.len() {
        return Err("eapol-key-data-test-wrap-large");
    }
    let mut a = [0xa6u8; 8];
    output[8..wrapped_len].copy_from_slice(plain);
    let cipher = Aes128::new(GenericArray::from_slice(kek));
    for j in 0..6 {
        for i in 1..=n {
            let t = (n * j + i) as u64;
            let mut block = [0u8; 16];
            block[..8].copy_from_slice(&a);
            block[8..].copy_from_slice(&output[8 + (i - 1) * 8..8 + i * 8]);
            cipher.encrypt_block(GenericArray::from_mut_slice(&mut block));
            a.copy_from_slice(&block[..8]);
            xor_key_wrap_t(&mut a, t);
            output[8 + (i - 1) * 8..8 + i * 8].copy_from_slice(&block[8..]);
        }
    }
    output[..8].copy_from_slice(&a);
    Ok(wrapped_len)
}

#[cfg(test)]
pub(crate) fn write_test_m1_frame(
    frame: &mut [u8; MAX_FRAME_LEN],
    station_mac: &[u8; ETHER_ADDR_LEN],
    ap_mac: &[u8; ETHER_ADDR_LEN],
) -> Result<usize, &'static str> {
    let mut replay_counter = [0u8; WPA_REPLAY_COUNTER_LEN];
    replay_counter[WPA_REPLAY_COUNTER_LEN - 1] = 1;
    let anonce = [
        0x10, 0x33, 0x56, 0x79, 0x9c, 0xbf, 0xe2, 0x05, 0x28, 0x4b, 0x6e, 0x91, 0xb4, 0xd7, 0xfa,
        0x1d, 0x40, 0x63, 0x86, 0xa9, 0xcc, 0xef, 0x12, 0x35, 0x58, 0x7b, 0x9e, 0xc1, 0xe4, 0x07,
        0x2a, 0x4d,
    ];
    let key_info = EAPOL_KEY_VERSION_HMAC_SHA1_AES | EAPOL_KEY_INFO_KEY_TYPE | EAPOL_KEY_INFO_ACK;
    let zero_kck = [0u8; WPA_KCK_LEN];
    let len = write_eapol_key_reply_frame(
        frame,
        station_mac,
        ap_mac,
        key_info,
        &replay_counter,
        Some(&anonce),
        &[],
        &zero_kck,
    )?;
    let body = ETH_HEADER_LEN + EAPOL_HEADER_LEN;
    frame[body + EAPOL_KEY_BODY_MIC_OFFSET..body + EAPOL_KEY_BODY_MIC_OFFSET + WPA_MIC_LEN].fill(0);
    Ok(len)
}

#[cfg(test)]
pub(crate) fn write_test_m3_frame(
    frame: &mut [u8; MAX_FRAME_LEN],
    station_mac: &[u8; ETHER_ADDR_LEN],
    state: &HostEapolState,
) -> Result<usize, &'static str> {
    write_test_m3_frame_with_gtk(frame, station_mac, state, true)
}

#[cfg(test)]
pub(crate) fn write_test_m3_frame_without_gtk(
    frame: &mut [u8; MAX_FRAME_LEN],
    station_mac: &[u8; ETHER_ADDR_LEN],
    state: &HostEapolState,
) -> Result<usize, &'static str> {
    write_test_m3_frame_with_gtk(frame, station_mac, state, false)
}

#[cfg(test)]
fn write_test_m3_frame_with_gtk(
    frame: &mut [u8; MAX_FRAME_LEN],
    station_mac: &[u8; ETHER_ADDR_LEN],
    state: &HostEapolState,
    include_gtk: bool,
) -> Result<usize, &'static str> {
    if !state.m2_sent {
        return Err("host-eapol-test-m3-before-m2");
    }
    let mut plain = [0u8; 64];
    let mut offset = 0usize;
    plain[offset..offset + WPA2_PSK_CCMP_RSN_IE.len()].copy_from_slice(&WPA2_PSK_CCMP_RSN_IE);
    offset += WPA2_PSK_CCMP_RSN_IE.len();
    if include_gtk {
        write_test_gtk_kde(&mut plain, &mut offset)?;
    }
    let plain_len = (offset + 7) & !7;

    let mut wrapped = [0u8; HOST_EAPOL_KEY_DATA_MAX_LEN];
    let wrapped_len = test_aes128_key_wrap(
        &state.ptk[WPA_KCK_LEN..WPA_KCK_LEN + WPA_KEK_LEN],
        &plain[..plain_len],
        &mut wrapped,
    )?;
    let mut replay_counter = state.m1_replay_counter;
    for byte in replay_counter.iter_mut().rev() {
        let (next, carry) = byte.overflowing_add(1);
        *byte = next;
        if !carry {
            break;
        }
    }
    let key_info = EAPOL_KEY_VERSION_HMAC_SHA1_AES
        | EAPOL_KEY_INFO_PAIRWISE_RECV_MASK
        | EAPOL_KEY_INFO_SECURE
        | EAPOL_KEY_INFO_ENCRYPTED_KEY_DATA;
    write_eapol_key_reply_frame(
        frame,
        station_mac,
        &state.ap_mac,
        key_info,
        &replay_counter,
        Some(&state.anonce),
        &wrapped[..wrapped_len],
        &state.ptk[..WPA_KCK_LEN],
    )
}

#[cfg(test)]
pub(crate) fn write_test_group_key_frame(
    frame: &mut [u8; MAX_FRAME_LEN],
    station_mac: &[u8; ETHER_ADDR_LEN],
    state: &HostEapolState,
) -> Result<usize, &'static str> {
    if !state.m4_sent || !state.ptk_installed {
        return Err("host-eapol-test-group-before-ptk");
    }
    let mut plain = [0u8; 64];
    let mut offset = 0usize;
    write_test_gtk_kde(&mut plain, &mut offset)?;
    let plain_len = (offset + 7) & !7;
    let mut wrapped = [0u8; HOST_EAPOL_KEY_DATA_MAX_LEN];
    let wrapped_len = test_aes128_key_wrap(
        &state.ptk[WPA_KCK_LEN..WPA_KCK_LEN + WPA_KEK_LEN],
        &plain[..plain_len],
        &mut wrapped,
    )?;
    let mut replay_counter = if state.group_replay_counter_valid {
        state.group_replay_counter
    } else {
        state.m3_replay_counter
    };
    for byte in replay_counter.iter_mut().rev() {
        let (next, carry) = byte.overflowing_add(1);
        *byte = next;
        if !carry {
            break;
        }
    }
    let key_info = EAPOL_KEY_VERSION_HMAC_SHA1_AES
        | EAPOL_KEY_INFO_ACK
        | EAPOL_KEY_INFO_MIC
        | EAPOL_KEY_INFO_SECURE
        | EAPOL_KEY_INFO_ENCRYPTED_KEY_DATA;
    write_eapol_key_reply_frame(
        frame,
        station_mac,
        &state.ap_mac,
        key_info,
        &replay_counter,
        None,
        &wrapped[..wrapped_len],
        &state.ptk[..WPA_KCK_LEN],
    )
}

#[cfg(test)]
fn write_test_gtk_kde(plain: &mut [u8], offset: &mut usize) -> Result<(), &'static str> {
    if plain.len().saturating_sub(*offset) < 24 {
        return Err("host-eapol-test-gtk-kde-len");
    }
    let gtk = [
        0x70, 0x71, 0x72, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0x7b, 0x7c, 0x7d, 0x7e,
        0x7f,
    ];
    plain[*offset] = 0xdd;
    plain[*offset + 1] = 22;
    plain[*offset + 2..*offset + 5].copy_from_slice(&RSN_KDE_OUI);
    plain[*offset + 5] = RSN_KDE_TYPE_GTK;
    plain[*offset + 6] = 1;
    plain[*offset + 7] = 0;
    plain[*offset + 8..*offset + 8 + gtk.len()].copy_from_slice(&gtk);
    *offset += 24;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pbkdf2_matches_ieee_vector() {
        let mut pmk = [0u8; WSEC_PMK_LEN];
        fill_wpa2_psk_pmk(b"IEEE", b"password", &mut pmk).expect("pbkdf2");
        assert_eq!(
            pmk,
            [
                0xf4, 0x2c, 0x6f, 0xc5, 0x2d, 0xf0, 0xeb, 0xef, 0x9e, 0xbb, 0x4b, 0x90, 0xb3, 0x8a,
                0x5f, 0x90, 0x2e, 0x83, 0xfe, 0x1b, 0x13, 0x5a, 0x70, 0xe2, 0x3a, 0xed, 0x76, 0x2e,
                0x97, 0x10, 0xa1, 0x2e,
            ]
        );
    }

    #[test]
    fn hex_psk_decodes_direct_pmk() {
        let mut pmk = [0u8; WSEC_PMK_LEN];
        fill_wpa2_psk_pmk(
            b"IEEE",
            b"000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
            &mut pmk,
        )
        .expect("hex pmk");

        assert_eq!(
            pmk,
            [
                0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d,
                0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b,
                0x1c, 0x1d, 0x1e, 0x1f,
            ]
        );
    }

    #[test]
    fn host_eapol_pairwise_shapes_match_old_good_rsn_path() {
        assert_eq!(WPA_PTK_PRF_LABEL, b"Pairwise key expansion\0");
        assert_eq!(WPA_SNONCE_LABEL_PREFIX, b"Cohesix host WPA SNonce ");
        assert_eq!(WPA_EAPOL_REPLY_KEY_LEN, 0);
        assert_eq!(EAPOL_KEY_BODY_RSC_OFFSET, 61);
        assert_eq!(
            EAPOL_KEY_INFO_PAIRWISE_RECV_MASK,
            EAPOL_KEY_INFO_KEY_TYPE
                | EAPOL_KEY_INFO_INSTALL
                | EAPOL_KEY_INFO_ACK
                | EAPOL_KEY_INFO_MIC
        );
        assert_eq!(
            EAPOL_KEY_INFO_GROUP_M2,
            EAPOL_KEY_VERSION_HMAC_SHA1_AES | EAPOL_KEY_INFO_MIC | EAPOL_KEY_INFO_SECURE
        );
    }

    #[test]
    fn wsec_key_payload_matches_broadcom_layout() {
        let key = [
            0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d,
            0x1e, 0x1f,
        ];
        let ea = [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff];
        let rsc = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06];
        let mut payload = [0xff; WSEC_KEY_PAYLOAD_LEN];
        let len = write_wsec_key_payload(&mut payload, 2, &key, &ea, Some(&rsc), true)
            .expect("wsec key payload");

        assert_eq!(len, WSEC_KEY_PAYLOAD_LEN);
        assert_eq!(
            u32::from_le_bytes([
                payload[WSEC_KEY_INDEX_OFFSET],
                payload[WSEC_KEY_INDEX_OFFSET + 1],
                payload[WSEC_KEY_INDEX_OFFSET + 2],
                payload[WSEC_KEY_INDEX_OFFSET + 3],
            ]),
            2
        );
        assert_eq!(
            u32::from_le_bytes([
                payload[WSEC_KEY_LEN_OFFSET],
                payload[WSEC_KEY_LEN_OFFSET + 1],
                payload[WSEC_KEY_LEN_OFFSET + 2],
                payload[WSEC_KEY_LEN_OFFSET + 3],
            ]),
            key.len() as u32
        );
        assert_eq!(
            &payload[WSEC_KEY_DATA_OFFSET..WSEC_KEY_DATA_OFFSET + key.len()],
            &key
        );
        assert_eq!(
            u32::from_le_bytes([
                payload[WSEC_KEY_ALGO_OFFSET],
                payload[WSEC_KEY_ALGO_OFFSET + 1],
                payload[WSEC_KEY_ALGO_OFFSET + 2],
                payload[WSEC_KEY_ALGO_OFFSET + 3],
            ]),
            CRYPTO_ALGO_AES_CCM
        );
        assert_eq!(
            u32::from_le_bytes([
                payload[WSEC_KEY_FLAGS_OFFSET],
                payload[WSEC_KEY_FLAGS_OFFSET + 1],
                payload[WSEC_KEY_FLAGS_OFFSET + 2],
                payload[WSEC_KEY_FLAGS_OFFSET + 3],
            ]),
            BRCMF_PRIMARY_KEY
        );
        assert_eq!(
            u32::from_le_bytes([
                payload[WSEC_KEY_IV_INITIALIZED_OFFSET],
                payload[WSEC_KEY_IV_INITIALIZED_OFFSET + 1],
                payload[WSEC_KEY_IV_INITIALIZED_OFFSET + 2],
                payload[WSEC_KEY_IV_INITIALIZED_OFFSET + 3],
            ]),
            1
        );
        assert_eq!(
            u32::from_le_bytes([
                payload[WSEC_KEY_RXIV_HI_OFFSET],
                payload[WSEC_KEY_RXIV_HI_OFFSET + 1],
                payload[WSEC_KEY_RXIV_HI_OFFSET + 2],
                payload[WSEC_KEY_RXIV_HI_OFFSET + 3],
            ]),
            0x0605_0403
        );
        assert_eq!(
            u16::from_le_bytes([
                payload[WSEC_KEY_RXIV_LO_OFFSET],
                payload[WSEC_KEY_RXIV_LO_OFFSET + 1],
            ]),
            0x0201
        );
        assert_eq!(
            &payload[WSEC_KEY_EA_OFFSET..WSEC_KEY_EA_OFFSET + ea.len()],
            &ea
        );
    }

    #[test]
    fn eapol_start_frame_matches_8021x_shape() {
        let dst = [0x01, 0x80, 0xc2, 0x00, 0x00, 0x03];
        let src = [0x02, 0x43, 0x4f, 0x48, 0x58, 0x32];
        let mut frame = [0xff; ETH_HEADER_LEN + EAPOL_HEADER_LEN];
        let len = write_eapol_start_frame(&mut frame, &dst, &src).expect("eapol start");

        assert_eq!(len, ETH_HEADER_LEN + EAPOL_HEADER_LEN);
        assert_eq!(&frame[..6], &dst);
        assert_eq!(&frame[6..12], &src);
        assert_eq!(u16::from_be_bytes([frame[12], frame[13]]), ETH_P_EAPOL);
        assert_eq!(frame[ETH_HEADER_LEN], EAPOL_VERSION_8021X_2004);
        assert_eq!(frame[ETH_HEADER_LEN + 1], EAPOL_PACKET_TYPE_START);
        assert_eq!(
            u16::from_be_bytes([frame[ETH_HEADER_LEN + 2], frame[ETH_HEADER_LEN + 3]]),
            0
        );
    }

    #[test]
    fn retransmitted_m1_rotates_m2_and_preserves_candidates() {
        let station = [0x88, 0xa2, 0x9e, 0x66, 0x59, 0x10];
        let ap = [0xf0, 0x72, 0xea, 0x4c, 0xc7, 0xa5];
        let mut state = HostEapolState::new(b"cohesix", b"passphrase").expect("host eapol");
        let mut m1 = [0u8; MAX_FRAME_LEN];
        let m1_len = write_test_m1_frame(&mut m1, &station, &ap).expect("m1");
        let mut first_m2 = [0u8; MAX_FRAME_LEN];
        let mut second_m2 = [0u8; MAX_FRAME_LEN];

        let first = state
            .handle_packet(station, &m1[..m1_len], &mut first_m2)
            .expect("first m1");
        let first_len = match first {
            HostEapolAction::SendM2 { len } => len,
            _ => panic!("first m1 should produce m2"),
        };
        let first_snonce = state.snonce;
        let first_ptk = state.ptk;
        let mut m3_for_first_m2 = [0u8; MAX_FRAME_LEN];
        let m3_len =
            write_test_m3_frame(&mut m3_for_first_m2, &station, &state).expect("m3 for first m2");

        let second = state
            .handle_packet(station, &m1[..m1_len], &mut second_m2)
            .expect("retransmitted m1");
        let second_len = match second {
            HostEapolAction::SendM2 { len } => len,
            _ => panic!("retransmitted m1 should produce m2"),
        };

        assert_eq!(state.rx_packets(), 2);
        assert_eq!(second_len, first_len);
        assert_ne!(state.snonce, first_snonce);
        assert_ne!(state.ptk, first_ptk);
        assert_ne!(&second_m2[..second_len], &first_m2[..first_len]);
        assert_eq!(state.ptk_candidate_count, 2);

        let m3_action = state
            .handle_packet(station, &m3_for_first_m2[..m3_len], &mut second_m2)
            .expect("m3 should match earlier m2 candidate");
        assert!(matches!(
            m3_action,
            HostEapolAction::SendM4InstallKeys { .. }
        ));
        assert_eq!(state.ptk, first_ptk);
        assert_eq!(state.snonce, first_snonce);
        assert!(state.secure_complete());
    }

    #[test]
    fn m3_can_match_earlier_m2_after_later_m1_updates_current_ptk() {
        let station = [0x88, 0xa2, 0x9e, 0x66, 0x59, 0x10];
        let ap = [0xf0, 0x72, 0xea, 0x4c, 0xc7, 0xa5];
        let mut state = HostEapolState::new(b"cohesix", b"passphrase").expect("host eapol");
        let mut first_m1 = [0u8; MAX_FRAME_LEN];
        let first_m1_len = write_test_m1_frame(&mut first_m1, &station, &ap).expect("m1");
        let mut tx = [0u8; MAX_FRAME_LEN];

        assert!(matches!(
            state
                .handle_packet(station, &first_m1[..first_m1_len], &mut tx)
                .expect("first m1"),
            HostEapolAction::SendM2 { .. }
        ));
        let first_ptk = state.ptk;
        let first_snonce = state.snonce;
        let mut m3_for_first_m2 = [0u8; MAX_FRAME_LEN];
        let m3_len =
            write_test_m3_frame(&mut m3_for_first_m2, &station, &state).expect("m3 for first m2");

        let mut later_m1 = first_m1;
        let body = ETH_HEADER_LEN + EAPOL_HEADER_LEN;
        later_m1[body + EAPOL_KEY_BODY_REPLAY_OFFSET + WPA_REPLAY_COUNTER_LEN - 1] = 2;
        assert!(matches!(
            state
                .handle_packet(station, &later_m1[..first_m1_len], &mut tx)
                .expect("later m1"),
            HostEapolAction::SendM2 { .. }
        ));
        assert_ne!(state.snonce, first_snonce);
        assert_ne!(state.ptk, first_ptk);
        assert_eq!(state.ptk_candidate_count, 2);

        let m3_action = state
            .handle_packet(station, &m3_for_first_m2[..m3_len], &mut tx)
            .expect("m3 should match earlier m2 candidate");
        assert!(matches!(
            m3_action,
            HostEapolAction::SendM4InstallKeys { .. }
        ));
        assert_eq!(state.ptk, first_ptk);
        assert_eq!(state.snonce, first_snonce);
        assert!(state.secure_complete());
    }

    #[test]
    fn pae_group_destination_is_admitted_for_host_eapol() {
        let station = [0x88, 0xa2, 0x9e, 0x66, 0x59, 0x10];
        let ap = [0xf0, 0x72, 0xea, 0x4c, 0xc7, 0xa5];
        let mut m1 = [0u8; MAX_FRAME_LEN];
        let m1_len = write_test_m1_frame(&mut m1, &PAE_GROUP_ADDR, &ap).expect("m1");

        assert!(packet_dst_allowed(&m1[..m1_len], station));
    }

    #[test]
    fn ptk_only_m3_waits_for_group_key_before_secure_complete() {
        let station = [0x88, 0xa2, 0x9e, 0x66, 0x59, 0x10];
        let ap = [0xf0, 0x72, 0xea, 0x4c, 0xc7, 0xa5];
        let mut state = HostEapolState::new(b"cohesix", b"passphrase").expect("host eapol");
        let mut m1 = [0u8; MAX_FRAME_LEN];
        let m1_len = write_test_m1_frame(&mut m1, &station, &ap).expect("m1");
        let mut tx = [0u8; MAX_FRAME_LEN];
        let m1_action = state
            .handle_packet(station, &m1[..m1_len], &mut tx)
            .expect("m1");
        assert!(matches!(m1_action, HostEapolAction::SendM2 { .. }));

        let mut m3 = [0u8; MAX_FRAME_LEN];
        let m3_len =
            write_test_m3_frame_without_gtk(&mut m3, &station, &state).expect("ptk-only m3");
        let m3_action = state
            .handle_packet(station, &m3[..m3_len], &mut tx)
            .expect("m3 without gtk");
        match m3_action {
            HostEapolAction::SendM4InstallKeys { keys, .. } => {
                assert!(keys.gtk.is_none());
            }
            _ => panic!("ptk-only m3 should send m4"),
        }
        assert!(state.m4_sent);
        assert!(state.ptk_installed);
        assert!(!state.gtk_installed);
        assert!(!state.secure_complete());

        let mut group = [0u8; MAX_FRAME_LEN];
        let group_len =
            write_test_group_key_frame(&mut group, &station, &state).expect("group key");
        let group_action = state
            .handle_packet(station, &group[..group_len], &mut tx)
            .expect("group key");

        assert!(matches!(
            group_action,
            HostEapolAction::SendGroupM2InstallGtk { .. }
        ));
        assert!(state.secure_complete());
    }

    #[test]
    fn retransmitted_m3_after_secure_resends_m4() {
        let station = [0x88, 0xa2, 0x9e, 0x66, 0x59, 0x10];
        let ap = [0xf0, 0x72, 0xea, 0x4c, 0xc7, 0xa5];
        let mut state = HostEapolState::new(b"cohesix", b"passphrase").expect("host eapol");
        let mut m1 = [0u8; MAX_FRAME_LEN];
        let m1_len = write_test_m1_frame(&mut m1, &station, &ap).expect("m1");
        let mut tx = [0u8; MAX_FRAME_LEN];
        assert!(matches!(
            state
                .handle_packet(station, &m1[..m1_len], &mut tx)
                .expect("m1"),
            HostEapolAction::SendM2 { .. }
        ));

        let mut m3 = [0u8; MAX_FRAME_LEN];
        let m3_len = write_test_m3_frame(&mut m3, &station, &state).expect("m3");
        assert!(matches!(
            state
                .handle_packet(station, &m3[..m3_len], &mut tx)
                .expect("first m3"),
            HostEapolAction::SendM4InstallKeys { .. }
        ));
        assert!(state.secure_complete());

        let retransmit = state
            .handle_packet(station, &m3[..m3_len], &mut tx)
            .expect("retransmitted m3 after secure");
        assert!(matches!(
            retransmit,
            HostEapolAction::SendM4InstallKeys { .. }
        ));
        assert!(state.secure_complete());
    }

    #[test]
    fn retransmitted_group_key_after_secure_resends_group_m2() {
        let station = [0x88, 0xa2, 0x9e, 0x66, 0x59, 0x10];
        let ap = [0xf0, 0x72, 0xea, 0x4c, 0xc7, 0xa5];
        let mut state = HostEapolState::new(b"cohesix", b"passphrase").expect("host eapol");
        let mut m1 = [0u8; MAX_FRAME_LEN];
        let m1_len = write_test_m1_frame(&mut m1, &station, &ap).expect("m1");
        let mut tx = [0u8; MAX_FRAME_LEN];
        assert!(matches!(
            state
                .handle_packet(station, &m1[..m1_len], &mut tx)
                .expect("m1"),
            HostEapolAction::SendM2 { .. }
        ));

        let mut m3 = [0u8; MAX_FRAME_LEN];
        let m3_len =
            write_test_m3_frame_without_gtk(&mut m3, &station, &state).expect("ptk-only m3");
        assert!(matches!(
            state
                .handle_packet(station, &m3[..m3_len], &mut tx)
                .expect("m3 without gtk"),
            HostEapolAction::SendM4InstallKeys { .. }
        ));

        let mut group = [0u8; MAX_FRAME_LEN];
        let group_len =
            write_test_group_key_frame(&mut group, &station, &state).expect("group key");
        assert!(matches!(
            state
                .handle_packet(station, &group[..group_len], &mut tx)
                .expect("first group key"),
            HostEapolAction::SendGroupM2InstallGtk { .. }
        ));
        assert!(state.secure_complete());

        let retransmit = state
            .handle_packet(station, &group[..group_len], &mut tx)
            .expect("retransmitted group key after secure");
        assert!(matches!(
            retransmit,
            HostEapolAction::SendGroupM2InstallGtk { .. }
        ));
        assert!(state.secure_complete());
    }

    #[test]
    fn aes_key_unwrap_matches_rfc3394_vector() {
        let kek = [
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d,
            0x0e, 0x0f,
        ];
        let wrapped = [
            0x1f, 0xa6, 0x8b, 0x0a, 0x81, 0x12, 0xb4, 0x47, 0xae, 0xf3, 0x4b, 0xd8, 0xfb, 0x5a,
            0x7b, 0x82, 0x9d, 0x3e, 0x86, 0x23, 0x71, 0xd2, 0xcf, 0xe5,
        ];
        let mut out = [0u8; HOST_EAPOL_KEY_DATA_MAX_LEN];
        let len = aes128_key_unwrap(&kek, &wrapped, &mut out).expect("unwrap");
        assert_eq!(len, 16);
        assert_eq!(
            &out[..16],
            [
                0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd,
                0xee, 0xff,
            ]
        );
    }
}
