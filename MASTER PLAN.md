Make factory better

The yield manager contract could maintain the high water mark system, and then the YT contract just calls the yield manager assuming no matter what the YM
contract maintains it properly. This means the YM contract would store the exchange rate, and return the max of the two rates, and update the stored rate if its
not the biggest rate.

Also, make the YT contract support any scalar size based on the underlying vault. right now its hardcoded to 1e6 because of the mock vault, but it can be different
scalar sizes for different vaults

Defindex



DEFINDEX TESTNET CONTRACTS
{
"ids": {
"defindex_factory": "CD6MEVYGXCCUTOUIC3GNMIDOSRY4A2WGCRQGOOCVG5PK2N7UNGGU6BBQ",
"xlm_hodl_strategy_0": "CCEE2VAGPXKVIZXTVIT4O5B7GCUDTZTJ5RIXBPJSZ7JWJCJ2TLK75WVW",
"xlm_hodl_strategy_1": "CAHWRPKBPX4FNLXZOAD565IBSICQPL5QX37IDLGJYOPWX22WWKFWQUBA",
"xlm_blend_autocompound_fixed_xlm_usdc_strategy": "CCSPRGGUP32M23CTU7RUAGXDNOHSA6O2BS2IK4NVUP5X2JQXKTSIQJKE",
"usdc_blend_autocompound_fixed_xlm_usdc_strategy": "CBLXUUHUL7TA3LF3U5G6ZTU7EACBBOSJLR4AYOM5YJKJ4APZ7O547R5T",
"xlm_hodl_vault": "CCGKL6U2DHSNFJ3NU4UPRUKYE2EUGYR4ZFZDYA7KDJLP3TKSPHD5C4UP"
},
"hashes": {
"defindex_vault": "ae3409a4090bc087b86b4e9b444d2b8017ccd97b90b069d44d005ab9f8e1468b",
"defindex_factory": "b0fe36b2b294d0af86846ccc4036279418907b60f6f74dae752847ae9d3bca0e",
"hodl_strategy": "c79eb65b4e890f4d8a2466bb2423b957c6c6ea7e490db64eed7e0118350d8967",
"blend_strategy": "11329c2469455f5a3815af1383c0cdddb69215b1668a17ef097516cde85da988"
}
}



MAINNET CONTRACTS
{
"ids": {
"defindex_factory": "CDKFHFJIET3A73A2YN4KV7NSV32S6YGQMUFH3DNJXLBWL4SKEGVRNFKI",
"usdc_blend_autocompound_fixed_strategy": "CDB2WMKQQNVZMEBY7Q7GZ5C7E7IAFSNMZ7GGVD6WKTCEWK7XOIAVZSAP",
"eurc_blend_autocompound_fixed_strategy": "CC5CE6MWISDXT3MLNQ7R3FVILFVFEIH3COWGH45GJKL6BD2ZHF7F7JVI",
"xlm_blend_autocompound_fixed_strategy": "CDPWNUW7UMCSVO36VAJSQHQECISPJLCVPDASKHRC5SEROAAZDUQ5DG2Z",
"usdc_blend_autocompound_yieldblox_strategy": "CCSRX5E4337QMCMC3KO3RDFYI57T5NZV5XB3W3TWE4USCASKGL5URKJL",
"eurc_blend_autocompound_yieldblox_strategy": "CA33NXYN7H3EBDSA3U2FPSULGJTTL3FQRHD2ADAAPTKS3FUJOE73735A",
"xlm_blend_autocompound_yieldblox_strategy": "CBDOIGFO2QOOZTWQZ7AFPH5JOUS2SBN5CTTXR665NHV6GOCM6OUGI5KP",
"cetes_blend_autocompound_yieldblox_strategy": "CBTSRJLN5CVVOWLTH2FY5KNQ47KW5KKU3VWGASDN72STGMXLRRNHPRIL",
"aqua_blend_autocompound_yieldblox_strategy": "CCMJUJW6Z7I3TYDCJFGTI3A7QA3ASMYAZ5PSRRWBBIJQPKI2GXL5DW5D",
"ustry_blend_autocompound_yieldblox_strategy": "CDDXPBOF727FDVTNV4I3G4LL4BHTJHE5BBC4W6WZAHMUPFDPBQBL6K7Y",
"usdglo_blend_autocompound_yieldblox_strategy": "CCTLQXYSIUN3OSZLZ7O7MIJC6YCU3QLLS6TUM3P2CD6DAVELMWC3QV4E",
"usdc_palta_vault":"CCFWKCD52JNSQLN5OS4F7EG6BPDT4IRJV6KODIEIZLWPM35IKHOKT6S2"
},
"hashes": {
"blend_strategy": "11329c2469455f5a3815af1383c0cdddb69215b1668a17ef097516cde85da988",
"defindex_vault": "ae3409a4090bc087b86b4e9b444d2b8017ccd97b90b069d44d005ab9f8e1468b",
"defindex_factory": "b0fe36b2b294d0af86846ccc4036279418907b60f6f74dae752847ae9d3bca0e"
}
}