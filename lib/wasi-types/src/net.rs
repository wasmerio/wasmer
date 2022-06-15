use super::*;
use wasmer_derive::ValueType;

use crate::__wasi_option_timestamp_t;

pub type __wasi_socktype_t = u16;
pub const __WASI_SOCK_TYPE_DGRAM: __wasi_socktype_t = 0;
pub const __WASI_SOCK_TYPE_STREAM: __wasi_socktype_t = 1;
pub const __WASI_SOCK_TYPE_RAW: __wasi_socktype_t = 2;
pub const __WASI_SOCK_TYPE_SEQPACKET: __wasi_socktype_t = 3;

pub type __wasi_sockstatus_t = u8;
pub const __WASI_SOCK_STATUS_OPENING: __wasi_sockstatus_t = 0;
pub const __WASI_SOCK_STATUS_OPENED: __wasi_sockstatus_t = 1;
pub const __WASI_SOCK_STATUS_CLOSED: __wasi_sockstatus_t = 2;
pub const __WASI_SOCK_STATUS_FAILED: __wasi_sockstatus_t = 3;

pub type __wasi_sockoption_t = u8;
pub const __WASI_SOCK_OPTION_NOOP: __wasi_sockoption_t = 0;
pub const __WASI_SOCK_OPTION_REUSE_PORT: __wasi_sockoption_t = 1;
pub const __WASI_SOCK_OPTION_REUSE_ADDR: __wasi_sockoption_t = 2;
pub const __WASI_SOCK_OPTION_NO_DELAY: __wasi_sockoption_t = 3;
pub const __WASI_SOCK_OPTION_DONT_ROUTE: __wasi_sockoption_t = 4;
pub const __WASI_SOCK_OPTION_ONLY_V6: __wasi_sockoption_t = 5;
pub const __WASI_SOCK_OPTION_BROADCAST: __wasi_sockoption_t = 6;
pub const __WASI_SOCK_OPTION_MULTICAST_LOOP_V4: __wasi_sockoption_t = 7;
pub const __WASI_SOCK_OPTION_MULTICAST_LOOP_V6: __wasi_sockoption_t = 8;
pub const __WASI_SOCK_OPTION_PROMISCUOUS: __wasi_sockoption_t = 9;
pub const __WASI_SOCK_OPTION_LISTENING: __wasi_sockoption_t = 10;
pub const __WASI_SOCK_OPTION_LAST_ERROR: __wasi_sockoption_t = 11;
pub const __WASI_SOCK_OPTION_KEEP_ALIVE: __wasi_sockoption_t = 12;
pub const __WASI_SOCK_OPTION_LINGER: __wasi_sockoption_t = 13;
pub const __WASI_SOCK_OPTION_OOB_INLINE: __wasi_sockoption_t = 14;
pub const __WASI_SOCK_OPTION_RECV_BUF_SIZE: __wasi_sockoption_t = 15;
pub const __WASI_SOCK_OPTION_SEND_BUF_SIZE: __wasi_sockoption_t = 16;
pub const __WASI_SOCK_OPTION_RECV_LOWAT: __wasi_sockoption_t = 17;
pub const __WASI_SOCK_OPTION_SEND_LOWAT: __wasi_sockoption_t = 18;
pub const __WASI_SOCK_OPTION_RECV_TIMEOUT: __wasi_sockoption_t = 19;
pub const __WASI_SOCK_OPTION_SEND_TIMEOUT: __wasi_sockoption_t = 20;
pub const __WASI_SOCK_OPTION_CONNECT_TIMEOUT: __wasi_sockoption_t = 21;
pub const __WASI_SOCK_OPTION_ACCEPT_TIMEOUT: __wasi_sockoption_t = 22;
pub const __WASI_SOCK_OPTION_TTL: __wasi_sockoption_t = 23;
pub const __WASI_SOCK_OPTION_MULTICAST_TTL_V4: __wasi_sockoption_t = 24;
pub const __WASI_SOCK_OPTION_TYPE: __wasi_sockoption_t = 25;
pub const __WASI_SOCK_OPTION_PROTO: __wasi_sockoption_t = 26;

pub type __wasi_streamsecurity_t = u8;
pub const __WASI_STREAM_SECURITY_UNENCRYPTED: __wasi_streamsecurity_t = 0;
pub const __WASI_STREAM_SECURITY_ANY_ENCRYPTION: __wasi_streamsecurity_t = 1;
pub const __WASI_STREAM_SECURITY_CLASSIC_ENCRYPTION: __wasi_streamsecurity_t = 2;
pub const __WASI_STREAM_SECURITY_DOUBLE_ENCRYPTION: __wasi_streamsecurity_t = 3;

pub type __wasi_sockproto_t = u16;
pub const __WASI_SOCK_PROTO_IP: __wasi_sockproto_t = 0;
pub const __WASI_SOCK_PROTO_ICMP: __wasi_sockproto_t = 1;
pub const __WASI_SOCK_PROTO_IGMP: __wasi_sockproto_t = 2;
pub const __WASI_SOCK_PROTO_PROTO_3: __wasi_sockproto_t = 3;
pub const __WASI_SOCK_PROTO_IPIP: __wasi_sockproto_t = 4;
pub const __WASI_SOCK_PROTO_PROTO_5: __wasi_sockproto_t = 5;
pub const __WASI_SOCK_PROTO_TCP: __wasi_sockproto_t = 6;
pub const __WASI_SOCK_PROTO_PROTO_7: __wasi_sockproto_t = 7;
pub const __WASI_SOCK_PROTO_EGP: __wasi_sockproto_t = 8;
pub const __WASI_SOCK_PROTO_PROTO_9: __wasi_sockproto_t = 9;
pub const __WASI_SOCK_PROTO_PROTO_10: __wasi_sockproto_t = 10;
pub const __WASI_SOCK_PROTO_PROTO_11: __wasi_sockproto_t = 11;
pub const __WASI_SOCK_PROTO_PUP: __wasi_sockproto_t = 12;
pub const __WASI_SOCK_PROTO_PROTO_13: __wasi_sockproto_t = 13;
pub const __WASI_SOCK_PROTO_PROTO_14: __wasi_sockproto_t = 14;
pub const __WASI_SOCK_PROTO_PROTO_15: __wasi_sockproto_t = 15;
pub const __WASI_SOCK_PROTO_PROTO_16: __wasi_sockproto_t = 16;
pub const __WASI_SOCK_PROTO_UDP: __wasi_sockproto_t = 17;
pub const __WASI_SOCK_PROTO_PROTO_18: __wasi_sockproto_t = 18;
pub const __WASI_SOCK_PROTO_PROTO_19: __wasi_sockproto_t = 19;
pub const __WASI_SOCK_PROTO_PROTO_20: __wasi_sockproto_t = 20;
pub const __WASI_SOCK_PROTO_PROTO_21: __wasi_sockproto_t = 21;
pub const __WASI_SOCK_PROTO_IDP: __wasi_sockproto_t = 22;
pub const __WASI_SOCK_PROTO_PROTO_23: __wasi_sockproto_t = 23;
pub const __WASI_SOCK_PROTO_PROTO_24: __wasi_sockproto_t = 24;
pub const __WASI_SOCK_PROTO_PROTO_25: __wasi_sockproto_t = 25;
pub const __WASI_SOCK_PROTO_PROTO_26: __wasi_sockproto_t = 26;
pub const __WASI_SOCK_PROTO_PROTO_27: __wasi_sockproto_t = 27;
pub const __WASI_SOCK_PROTO_PROTO_28: __wasi_sockproto_t = 28;
pub const __WASI_SOCK_PROTO_PROTO_TP: __wasi_sockproto_t = 29;
pub const __WASI_SOCK_PROTO_PROTO_30: __wasi_sockproto_t = 30;
pub const __WASI_SOCK_PROTO_PROTO_31: __wasi_sockproto_t = 31;
pub const __WASI_SOCK_PROTO_PROTO_32: __wasi_sockproto_t = 32;
pub const __WASI_SOCK_PROTO_DCCP: __wasi_sockproto_t = 33;
pub const __WASI_SOCK_PROTO_PROTO_34: __wasi_sockproto_t = 34;
pub const __WASI_SOCK_PROTO_PROTO_35: __wasi_sockproto_t = 35;
pub const __WASI_SOCK_PROTO_PROTO_36: __wasi_sockproto_t = 36;
pub const __WASI_SOCK_PROTO_PROTO_37: __wasi_sockproto_t = 37;
pub const __WASI_SOCK_PROTO_PROTO_38: __wasi_sockproto_t = 38;
pub const __WASI_SOCK_PROTO_PROTO_39: __wasi_sockproto_t = 39;
pub const __WASI_SOCK_PROTO_PROTO_40: __wasi_sockproto_t = 40;
pub const __WASI_SOCK_PROTO_IPV6: __wasi_sockproto_t = 41;
pub const __WASI_SOCK_PROTO_PROTO_42: __wasi_sockproto_t = 42;
pub const __WASI_SOCK_PROTO_ROUTING: __wasi_sockproto_t = 43;
pub const __WASI_SOCK_PROTO_FRAGMENT: __wasi_sockproto_t = 44;
pub const __WASI_SOCK_PROTO_PROTO_45: __wasi_sockproto_t = 45;
pub const __WASI_SOCK_PROTO_RSVP: __wasi_sockproto_t = 46;
pub const __WASI_SOCK_PROTO_GRE: __wasi_sockproto_t = 47;
pub const __WASI_SOCK_PROTO_PROTO_48: __wasi_sockproto_t = 48;
pub const __WASI_SOCK_PROTO_PROTO_49: __wasi_sockproto_t = 49;
pub const __WASI_SOCK_PROTO_ESP: __wasi_sockproto_t = 50;
pub const __WASI_SOCK_PROTO_AH: __wasi_sockproto_t = 51;
pub const __WASI_SOCK_PROTO_PROTO_52: __wasi_sockproto_t = 52;
pub const __WASI_SOCK_PROTO_PROTO_53: __wasi_sockproto_t = 53;
pub const __WASI_SOCK_PROTO_PROTO_54: __wasi_sockproto_t = 54;
pub const __WASI_SOCK_PROTO_PROTO_55: __wasi_sockproto_t = 55;
pub const __WASI_SOCK_PROTO_PROTO_56: __wasi_sockproto_t = 56;
pub const __WASI_SOCK_PROTO_PROTO_57: __wasi_sockproto_t = 57;
pub const __WASI_SOCK_PROTO_ICMPV6: __wasi_sockproto_t = 58;
pub const __WASI_SOCK_PROTO_NONE: __wasi_sockproto_t = 59;
pub const __WASI_SOCK_PROTO_DSTOPTS: __wasi_sockproto_t = 60;
pub const __WASI_SOCK_PROTO_PROTO_61: __wasi_sockproto_t = 61;
pub const __WASI_SOCK_PROTO_PROTO_62: __wasi_sockproto_t = 62;
pub const __WASI_SOCK_PROTO_PROTO_63: __wasi_sockproto_t = 63;
pub const __WASI_SOCK_PROTO_PROTO_64: __wasi_sockproto_t = 64;
pub const __WASI_SOCK_PROTO_PROTO_65: __wasi_sockproto_t = 65;
pub const __WASI_SOCK_PROTO_PROTO_66: __wasi_sockproto_t = 66;
pub const __WASI_SOCK_PROTO_PROTO_67: __wasi_sockproto_t = 67;
pub const __WASI_SOCK_PROTO_PROTO_68: __wasi_sockproto_t = 68;
pub const __WASI_SOCK_PROTO_PROTO_69: __wasi_sockproto_t = 69;
pub const __WASI_SOCK_PROTO_PROTO_70: __wasi_sockproto_t = 70;
pub const __WASI_SOCK_PROTO_PROTO_71: __wasi_sockproto_t = 71;
pub const __WASI_SOCK_PROTO_PROTO_72: __wasi_sockproto_t = 72;
pub const __WASI_SOCK_PROTO_PROTO_73: __wasi_sockproto_t = 73;
pub const __WASI_SOCK_PROTO_PROTO_74: __wasi_sockproto_t = 74;
pub const __WASI_SOCK_PROTO_PROTO_75: __wasi_sockproto_t = 75;
pub const __WASI_SOCK_PROTO_PROTO_76: __wasi_sockproto_t = 76;
pub const __WASI_SOCK_PROTO_PROTO_77: __wasi_sockproto_t = 77;
pub const __WASI_SOCK_PROTO_PROTO_78: __wasi_sockproto_t = 78;
pub const __WASI_SOCK_PROTO_PROTO_79: __wasi_sockproto_t = 79;
pub const __WASI_SOCK_PROTO_PROTO_80: __wasi_sockproto_t = 80;
pub const __WASI_SOCK_PROTO_PROTO_81: __wasi_sockproto_t = 81;
pub const __WASI_SOCK_PROTO_PROTO_82: __wasi_sockproto_t = 82;
pub const __WASI_SOCK_PROTO_PROTO_83: __wasi_sockproto_t = 83;
pub const __WASI_SOCK_PROTO_PROTO_84: __wasi_sockproto_t = 84;
pub const __WASI_SOCK_PROTO_PROTO_85: __wasi_sockproto_t = 85;
pub const __WASI_SOCK_PROTO_PROTO_86: __wasi_sockproto_t = 86;
pub const __WASI_SOCK_PROTO_PROTO_87: __wasi_sockproto_t = 87;
pub const __WASI_SOCK_PROTO_PROTO_88: __wasi_sockproto_t = 88;
pub const __WASI_SOCK_PROTO_PROTO_89: __wasi_sockproto_t = 89;
pub const __WASI_SOCK_PROTO_PROTO_90: __wasi_sockproto_t = 90;
pub const __WASI_SOCK_PROTO_PROTO_91: __wasi_sockproto_t = 91;
pub const __WASI_SOCK_PROTO_MTP: __wasi_sockproto_t = 92;
pub const __WASI_SOCK_PROTO_PROTO_93: __wasi_sockproto_t = 93;
pub const __WASI_SOCK_PROTO_BEETPH: __wasi_sockproto_t = 94;
pub const __WASI_SOCK_PROTO_PROTO_95: __wasi_sockproto_t = 95;
pub const __WASI_SOCK_PROTO_PROTO_96: __wasi_sockproto_t = 96;
pub const __WASI_SOCK_PROTO_PROTO_97: __wasi_sockproto_t = 97;
pub const __WASI_SOCK_PROTO_ENCAP: __wasi_sockproto_t = 98;
pub const __WASI_SOCK_PROTO_PROTO_99: __wasi_sockproto_t = 99;
pub const __WASI_SOCK_PROTO_PROTO_100: __wasi_sockproto_t = 100;
pub const __WASI_SOCK_PROTO_PROTO_101: __wasi_sockproto_t = 101;
pub const __WASI_SOCK_PROTO_PROTO_102: __wasi_sockproto_t = 102;
pub const __WASI_SOCK_PROTO_PIM: __wasi_sockproto_t = 103;
pub const __WASI_SOCK_PROTO_PROTO_104: __wasi_sockproto_t = 104;
pub const __WASI_SOCK_PROTO_PROTO_105: __wasi_sockproto_t = 105;
pub const __WASI_SOCK_PROTO_PROTO_106: __wasi_sockproto_t = 106;
pub const __WASI_SOCK_PROTO_PROTO_107: __wasi_sockproto_t = 107;
pub const __WASI_SOCK_PROTO_COMP: __wasi_sockproto_t = 108;
pub const __WASI_SOCK_PROTO_PROTO_109: __wasi_sockproto_t = 109;
pub const __WASI_SOCK_PROTO_PROTO_110: __wasi_sockproto_t = 110;
pub const __WASI_SOCK_PROTO_PROTO_111: __wasi_sockproto_t = 111;
pub const __WASI_SOCK_PROTO_PROTO_112: __wasi_sockproto_t = 112;
pub const __WASI_SOCK_PROTO_PROTO_113: __wasi_sockproto_t = 113;
pub const __WASI_SOCK_PROTO_PROTO_114: __wasi_sockproto_t = 114;
pub const __WASI_SOCK_PROTO_PROTO_115: __wasi_sockproto_t = 115;
pub const __WASI_SOCK_PROTO_PROTO_116: __wasi_sockproto_t = 116;
pub const __WASI_SOCK_PROTO_PROTO_117: __wasi_sockproto_t = 117;
pub const __WASI_SOCK_PROTO_PROTO_118: __wasi_sockproto_t = 118;
pub const __WASI_SOCK_PROTO_PROTO_119: __wasi_sockproto_t = 119;
pub const __WASI_SOCK_PROTO_PROTO_120: __wasi_sockproto_t = 120;
pub const __WASI_SOCK_PROTO_PROTO_121: __wasi_sockproto_t = 121;
pub const __WASI_SOCK_PROTO_PROTO_122: __wasi_sockproto_t = 122;
pub const __WASI_SOCK_PROTO_PROTO_123: __wasi_sockproto_t = 123;
pub const __WASI_SOCK_PROTO_PROTO_124: __wasi_sockproto_t = 124;
pub const __WASI_SOCK_PROTO_PROTO_125: __wasi_sockproto_t = 125;
pub const __WASI_SOCK_PROTO_PROTO_126: __wasi_sockproto_t = 126;
pub const __WASI_SOCK_PROTO_PROTO_127: __wasi_sockproto_t = 127;
pub const __WASI_SOCK_PROTO_PROTO_128: __wasi_sockproto_t = 128;
pub const __WASI_SOCK_PROTO_PROTO_129: __wasi_sockproto_t = 129;
pub const __WASI_SOCK_PROTO_PROTO_130: __wasi_sockproto_t = 130;
pub const __WASI_SOCK_PROTO_PROTO_131: __wasi_sockproto_t = 131;
pub const __WASI_SOCK_PROTO_SCTP: __wasi_sockproto_t = 132;
pub const __WASI_SOCK_PROTO_PROTO_133: __wasi_sockproto_t = 133;
pub const __WASI_SOCK_PROTO_PROTO_134: __wasi_sockproto_t = 134;
pub const __WASI_SOCK_PROTO_MH: __wasi_sockproto_t = 135;
pub const __WASI_SOCK_PROTO_UDPLITE: __wasi_sockproto_t = 136;
pub const __WASI_SOCK_PROTO_MPLS: __wasi_sockproto_t = 137;
pub const __WASI_SOCK_PROTO_PROTO_138: __wasi_sockproto_t = 138;
pub const __WASI_SOCK_PROTO_PROTO_139: __wasi_sockproto_t = 139;
pub const __WASI_SOCK_PROTO_PROTO_140: __wasi_sockproto_t = 140;
pub const __WASI_SOCK_PROTO_PROTO_141: __wasi_sockproto_t = 141;
pub const __WASI_SOCK_PROTO_PROTO_142: __wasi_sockproto_t = 142;
pub const __WASI_SOCK_PROTO_ETHERNET: __wasi_sockproto_t = 143;
pub const __WASI_SOCK_PROTO_PROTO_144: __wasi_sockproto_t = 144;
pub const __WASI_SOCK_PROTO_PROTO_145: __wasi_sockproto_t = 145;
pub const __WASI_SOCK_PROTO_PROTO_146: __wasi_sockproto_t = 146;
pub const __WASI_SOCK_PROTO_PROTO_147: __wasi_sockproto_t = 147;
pub const __WASI_SOCK_PROTO_PROTO_148: __wasi_sockproto_t = 148;
pub const __WASI_SOCK_PROTO_PROTO_149: __wasi_sockproto_t = 149;
pub const __WASI_SOCK_PROTO_PROTO_150: __wasi_sockproto_t = 150;
pub const __WASI_SOCK_PROTO_PROTO_151: __wasi_sockproto_t = 151;
pub const __WASI_SOCK_PROTO_PROTO_152: __wasi_sockproto_t = 152;
pub const __WASI_SOCK_PROTO_PROTO_153: __wasi_sockproto_t = 153;
pub const __WASI_SOCK_PROTO_PROTO_154: __wasi_sockproto_t = 154;
pub const __WASI_SOCK_PROTO_PROTO_155: __wasi_sockproto_t = 155;
pub const __WASI_SOCK_PROTO_PROTO_156: __wasi_sockproto_t = 156;
pub const __WASI_SOCK_PROTO_PROTO_157: __wasi_sockproto_t = 157;
pub const __WASI_SOCK_PROTO_PROTO_158: __wasi_sockproto_t = 158;
pub const __WASI_SOCK_PROTO_PROTO_159: __wasi_sockproto_t = 159;
pub const __WASI_SOCK_PROTO_PROTO_160: __wasi_sockproto_t = 160;
pub const __WASI_SOCK_PROTO_PROTO_161: __wasi_sockproto_t = 161;
pub const __WASI_SOCK_PROTO_PROTO_162: __wasi_sockproto_t = 162;
pub const __WASI_SOCK_PROTO_PROTO_163: __wasi_sockproto_t = 163;
pub const __WASI_SOCK_PROTO_PROTO_164: __wasi_sockproto_t = 164;
pub const __WASI_SOCK_PROTO_PROTO_165: __wasi_sockproto_t = 165;
pub const __WASI_SOCK_PROTO_PROTO_166: __wasi_sockproto_t = 166;
pub const __WASI_SOCK_PROTO_PROTO_167: __wasi_sockproto_t = 167;
pub const __WASI_SOCK_PROTO_PROTO_168: __wasi_sockproto_t = 168;
pub const __WASI_SOCK_PROTO_PROTO_169: __wasi_sockproto_t = 169;
pub const __WASI_SOCK_PROTO_PROTO_170: __wasi_sockproto_t = 170;
pub const __WASI_SOCK_PROTO_PROTO_171: __wasi_sockproto_t = 171;
pub const __WASI_SOCK_PROTO_PROTO_172: __wasi_sockproto_t = 172;
pub const __WASI_SOCK_PROTO_PROTO_173: __wasi_sockproto_t = 173;
pub const __WASI_SOCK_PROTO_PROTO_174: __wasi_sockproto_t = 174;
pub const __WASI_SOCK_PROTO_PROTO_175: __wasi_sockproto_t = 175;
pub const __WASI_SOCK_PROTO_PROTO_176: __wasi_sockproto_t = 176;
pub const __WASI_SOCK_PROTO_PROTO_177: __wasi_sockproto_t = 177;
pub const __WASI_SOCK_PROTO_PROTO_178: __wasi_sockproto_t = 178;
pub const __WASI_SOCK_PROTO_PROTO_179: __wasi_sockproto_t = 179;
pub const __WASI_SOCK_PROTO_PROTO_180: __wasi_sockproto_t = 180;
pub const __WASI_SOCK_PROTO_PROTO_181: __wasi_sockproto_t = 181;
pub const __WASI_SOCK_PROTO_PROTO_182: __wasi_sockproto_t = 182;
pub const __WASI_SOCK_PROTO_PROTO_183: __wasi_sockproto_t = 183;
pub const __WASI_SOCK_PROTO_PROTO_184: __wasi_sockproto_t = 184;
pub const __WASI_SOCK_PROTO_PROTO_185: __wasi_sockproto_t = 185;
pub const __WASI_SOCK_PROTO_PROTO_186: __wasi_sockproto_t = 186;
pub const __WASI_SOCK_PROTO_PROTO_187: __wasi_sockproto_t = 187;
pub const __WASI_SOCK_PROTO_PROTO_188: __wasi_sockproto_t = 188;
pub const __WASI_SOCK_PROTO_PROTO_189: __wasi_sockproto_t = 189;
pub const __WASI_SOCK_PROTO_PROTO_190: __wasi_sockproto_t = 190;
pub const __WASI_SOCK_PROTO_PROTO_191: __wasi_sockproto_t = 191;
pub const __WASI_SOCK_PROTO_PROTO_192: __wasi_sockproto_t = 192;
pub const __WASI_SOCK_PROTO_PROTO_193: __wasi_sockproto_t = 193;
pub const __WASI_SOCK_PROTO_PROTO_194: __wasi_sockproto_t = 194;
pub const __WASI_SOCK_PROTO_PROTO_195: __wasi_sockproto_t = 195;
pub const __WASI_SOCK_PROTO_PROTO_196: __wasi_sockproto_t = 196;
pub const __WASI_SOCK_PROTO_PROTO_197: __wasi_sockproto_t = 197;
pub const __WASI_SOCK_PROTO_PROTO_198: __wasi_sockproto_t = 198;
pub const __WASI_SOCK_PROTO_PROTO_199: __wasi_sockproto_t = 199;
pub const __WASI_SOCK_PROTO_PROTO_200: __wasi_sockproto_t = 200;
pub const __WASI_SOCK_PROTO_PROTO_201: __wasi_sockproto_t = 201;
pub const __WASI_SOCK_PROTO_PROTO_202: __wasi_sockproto_t = 202;
pub const __WASI_SOCK_PROTO_PROTO_203: __wasi_sockproto_t = 203;
pub const __WASI_SOCK_PROTO_PROTO_204: __wasi_sockproto_t = 204;
pub const __WASI_SOCK_PROTO_PROTO_205: __wasi_sockproto_t = 205;
pub const __WASI_SOCK_PROTO_PROTO_206: __wasi_sockproto_t = 206;
pub const __WASI_SOCK_PROTO_PROTO_207: __wasi_sockproto_t = 207;
pub const __WASI_SOCK_PROTO_PROTO_208: __wasi_sockproto_t = 208;
pub const __WASI_SOCK_PROTO_PROTO_209: __wasi_sockproto_t = 209;
pub const __WASI_SOCK_PROTO_PROTO_210: __wasi_sockproto_t = 210;
pub const __WASI_SOCK_PROTO_PROTO_211: __wasi_sockproto_t = 211;
pub const __WASI_SOCK_PROTO_PROTO_212: __wasi_sockproto_t = 212;
pub const __WASI_SOCK_PROTO_PROTO_213: __wasi_sockproto_t = 213;
pub const __WASI_SOCK_PROTO_PROTO_214: __wasi_sockproto_t = 214;
pub const __WASI_SOCK_PROTO_PROTO_215: __wasi_sockproto_t = 215;
pub const __WASI_SOCK_PROTO_PROTO_216: __wasi_sockproto_t = 216;
pub const __WASI_SOCK_PROTO_PROTO_217: __wasi_sockproto_t = 217;
pub const __WASI_SOCK_PROTO_PROTO_218: __wasi_sockproto_t = 218;
pub const __WASI_SOCK_PROTO_PROTO_219: __wasi_sockproto_t = 219;
pub const __WASI_SOCK_PROTO_PROTO_220: __wasi_sockproto_t = 220;
pub const __WASI_SOCK_PROTO_PROTO_221: __wasi_sockproto_t = 221;
pub const __WASI_SOCK_PROTO_PROTO_222: __wasi_sockproto_t = 222;
pub const __WASI_SOCK_PROTO_PROTO_223: __wasi_sockproto_t = 223;
pub const __WASI_SOCK_PROTO_PROTO_224: __wasi_sockproto_t = 224;
pub const __WASI_SOCK_PROTO_PROTO_225: __wasi_sockproto_t = 225;
pub const __WASI_SOCK_PROTO_PROTO_226: __wasi_sockproto_t = 226;
pub const __WASI_SOCK_PROTO_PROTO_227: __wasi_sockproto_t = 227;
pub const __WASI_SOCK_PROTO_PROTO_228: __wasi_sockproto_t = 228;
pub const __WASI_SOCK_PROTO_PROTO_229: __wasi_sockproto_t = 229;
pub const __WASI_SOCK_PROTO_PROTO_230: __wasi_sockproto_t = 230;
pub const __WASI_SOCK_PROTO_PROTO_231: __wasi_sockproto_t = 231;
pub const __WASI_SOCK_PROTO_PROTO_232: __wasi_sockproto_t = 232;
pub const __WASI_SOCK_PROTO_PROTO_233: __wasi_sockproto_t = 233;
pub const __WASI_SOCK_PROTO_PROTO_234: __wasi_sockproto_t = 234;
pub const __WASI_SOCK_PROTO_PROTO_235: __wasi_sockproto_t = 235;
pub const __WASI_SOCK_PROTO_PROTO_236: __wasi_sockproto_t = 236;
pub const __WASI_SOCK_PROTO_PROTO_237: __wasi_sockproto_t = 237;
pub const __WASI_SOCK_PROTO_PROTO_238: __wasi_sockproto_t = 238;
pub const __WASI_SOCK_PROTO_PROTO_239: __wasi_sockproto_t = 239;
pub const __WASI_SOCK_PROTO_PROTO_240: __wasi_sockproto_t = 240;
pub const __WASI_SOCK_PROTO_PROTO_241: __wasi_sockproto_t = 241;
pub const __WASI_SOCK_PROTO_PROTO_242: __wasi_sockproto_t = 242;
pub const __WASI_SOCK_PROTO_PROTO_243: __wasi_sockproto_t = 243;
pub const __WASI_SOCK_PROTO_PROTO_244: __wasi_sockproto_t = 244;
pub const __WASI_SOCK_PROTO_PROTO_245: __wasi_sockproto_t = 245;
pub const __WASI_SOCK_PROTO_PROTO_246: __wasi_sockproto_t = 246;
pub const __WASI_SOCK_PROTO_PROTO_247: __wasi_sockproto_t = 247;
pub const __WASI_SOCK_PROTO_PROTO_248: __wasi_sockproto_t = 248;
pub const __WASI_SOCK_PROTO_PROTO_249: __wasi_sockproto_t = 249;
pub const __WASI_SOCK_PROTO_PROTO_250: __wasi_sockproto_t = 250;
pub const __WASI_SOCK_PROTO_PROTO_251: __wasi_sockproto_t = 251;
pub const __WASI_SOCK_PROTO_PROTO_252: __wasi_sockproto_t = 252;
pub const __WASI_SOCK_PROTO_PROTO_253: __wasi_sockproto_t = 253;
pub const __WASI_SOCK_PROTO_PROTO_254: __wasi_sockproto_t = 254;
pub const __WASI_SOCK_PROTO_PROTO_RAW: __wasi_sockproto_t = 255;
pub const __WASI_SOCK_PROTO_PROTO_256: __wasi_sockproto_t = 256;
pub const __WASI_SOCK_PROTO_PROTO_257: __wasi_sockproto_t = 257;
pub const __WASI_SOCK_PROTO_PROTO_258: __wasi_sockproto_t = 258;
pub const __WASI_SOCK_PROTO_PROTO_259: __wasi_sockproto_t = 259;
pub const __WASI_SOCK_PROTO_PROTO_260: __wasi_sockproto_t = 260;
pub const __WASI_SOCK_PROTO_PROTO_261: __wasi_sockproto_t = 261;
pub const __WASI_SOCK_PROTO_MPTCP: __wasi_sockproto_t = 262;
pub const __WASI_SOCK_PROTO_MAX: __wasi_sockproto_t = 263;

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
#[repr(C)]
pub struct __wasi_hardwareaddress_t {
    pub octs: [u8; 6],
}

pub type __wasi_addressfamily_t = u16;
pub const __WASI_ADDRESS_FAMILY_UNSPEC: __wasi_addressfamily_t = 0;
pub const __WASI_ADDRESS_FAMILY_INET4: __wasi_addressfamily_t = 1;
pub const __WASI_ADDRESS_FAMILY_INET6: __wasi_addressfamily_t = 2;
pub const __WASI_ADDRESS_FAMILY_UNIX: __wasi_addressfamily_t = 3;

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
#[repr(C)]
pub struct __wasi_addr_unspec_t {
    pub n0: u8,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
#[repr(C)]
pub struct __wasi_addr_unspec_port_t {
    pub port: u16,
    pub addr: __wasi_addr_unspec_t,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
#[repr(C)]
pub struct __wasi_cidr_unspec_t {
    pub addr: __wasi_addr_unspec_t,
    pub prefix: u8,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
#[repr(C)]
pub struct __wasi_addr_ip4_t {
    pub octs: [u8; 4],
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
#[repr(C)]
pub struct __wasi_addr_ip4_port_t {
    pub port: u16,
    pub ip: __wasi_addr_ip4_t,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
#[repr(C)]
pub struct __wasi_cidr_ip4_t {
    pub ip: __wasi_addr_ip4_t,
    pub prefix: u8,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
#[repr(C)]
pub struct __wasi_addr_unix_t {
    pub octs: [u8; 16],
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
#[repr(C)]
pub struct __wasi_addr_unix_port_t {
    pub port: u16,
    pub unix: __wasi_addr_unix_t,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
#[repr(C)]
pub struct __wasi_cidr_unix_t {
    pub unix: __wasi_addr_unix_t,
    pub prefix: u8,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
#[repr(C)]
pub struct __wasi_addr_ip6_t {
    pub segs: [u8; 16],
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
#[repr(C)]
pub struct __wasi_addr_ip6_port_t {
    pub port: u16,
    pub ip: __wasi_addr_ip6_t,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
#[repr(C)]
pub struct __wasi_cidr_ip6_t {
    pub ip: __wasi_addr_ip6_t,
    pub prefix: u8,
}

#[derive(Debug, Copy, Clone, ValueType)]
#[repr(C)]
pub struct __wasi_addr_u {
    pub octs: [u8; 16],
}

#[derive(Debug, Copy, Clone, ValueType)]
#[repr(C)]
pub struct __wasi_addr_t {
    pub tag: __wasi_addressfamily_t,
    pub u: __wasi_addr_u,
}

#[derive(Debug, Copy, Clone, ValueType)]
#[repr(C)]
pub struct __wasi_addr_port_u {
    pub octs: [u8; 18],
}

#[derive(Debug, Copy, Clone, ValueType)]
#[repr(C)]
pub struct __wasi_addr_port_t {
    pub tag: __wasi_addressfamily_t,
    pub u: __wasi_addr_port_u,
}

#[derive(Debug, Copy, Clone, ValueType)]
#[repr(C)]
pub struct __wasi_cidr_u {
    pub octs: [u8; 17],
}

#[derive(Debug, Copy, Clone, ValueType)]
#[repr(C)]
pub struct __wasi_cidr_t {
    pub tag: __wasi_addressfamily_t,
    pub u: __wasi_cidr_u,
}

#[derive(Debug, Copy, Clone, ValueType)]
#[repr(C)]
pub struct __wasi_route_t {
    pub cidr: __wasi_cidr_t,
    pub via_router: __wasi_addr_t,
    pub preferred_until: __wasi_option_timestamp_t,
    pub expires_at: __wasi_option_timestamp_t,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
#[repr(C)]
pub struct __wasi_http_handles_t {
    pub req: __wasi_fd_t,
    pub res: __wasi_fd_t,
    pub hdr: __wasi_fd_t,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
#[repr(C)]
pub struct __wasi_http_status_t {
    pub ok: __wasi_bool_t,
    pub redirect: __wasi_bool_t,
    pub size: __wasi_filesize_t,
    pub status: u16,
}

pub type __wasi_riflags_t = u16;
pub const __WASI_SOCK_RECV_INPUT_PEEK: __wasi_riflags_t = 1 << 0;
pub const __WASI_SOCK_RECV_INPUT_WAITALL: __wasi_riflags_t = 1 << 1;
pub const __WASI_SOCK_RECV_INPUT_DATA_TRUNCATED: __wasi_riflags_t = 1 << 2;

pub type __wasi_roflags_t = u16;
pub const __WASI_SOCK_RECV_OUTPUT_DATA_TRUNCATED: __wasi_roflags_t = 1 << 0;

pub type __wasi_sdflags_t = u8;
pub const __WASI_SHUT_RD: __wasi_sdflags_t = 1 << 0;
pub const __WASI_SHUT_WR: __wasi_sdflags_t = 1 << 1;

pub type __wasi_siflags_t = u16;

pub type __wasi_timeout_t = u8;
pub const __WASI_TIMEOUT_READ: __wasi_timeout_t = 0;
pub const __WASI_TIMEOUT_WRITE: __wasi_timeout_t = 1;
pub const __WASI_TIMEOUT_CONNECT: __wasi_timeout_t = 2;
pub const __WASI_TIMEOUT_ACCEPT: __wasi_timeout_t = 3;
