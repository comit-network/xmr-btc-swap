use rand_core::OsRng;
use zeroize::Zeroizing;

use curve25519_dalek::scalar::Scalar;

use monero_primitives::keccak256;

use crate::*;

#[test]
fn test_original_seed() {
    struct Vector {
        language: Language,
        seed: String,
        spend: String,
        view: String,
    }

    let vectors = [
        Vector {
            language: Language::Chinese,
            seed: "摇 曲 艺 武 滴 然 效 似 赏 式 祥 歌 买 疑 小 碧 堆 博 键 房 鲜 悲 付 喷 武"
                .into(),
            spend: "a5e4fff1706ef9212993a69f246f5c95ad6d84371692d63e9bb0ea112a58340d".into(),
            view: "1176c43ce541477ea2f3ef0b49b25112b084e26b8a843e1304ac4677b74cdf02".into(),
        },
        Vector {
            language: Language::English,
            seed: "washing thirsty occur lectures tuesday fainted toxic adapt \
               abnormal memoir nylon mostly building shrugged online ember northern \
               ruby woes dauntless boil family illness inroads northern"
                .into(),
            spend: "c0af65c0dd837e666b9d0dfed62745f4df35aed7ea619b2798a709f0fe545403".into(),
            view: "513ba91c538a5a9069e0094de90e927c0cd147fa10428ce3ac1afd49f63e3b01".into(),
        },
        Vector {
            language: Language::Dutch,
            seed: "setwinst riphagen vimmetje extase blief tuitelig fuiven meifeest \
               ponywagen zesmaal ripdeal matverf codetaal leut ivoor rotten \
               wisgerhof winzucht typograaf atrium rein zilt traktaat verzaagd setwinst"
                .into(),
            spend: "e2d2873085c447c2bc7664222ac8f7d240df3aeac137f5ff2022eaa629e5b10a".into(),
            view: "eac30b69477e3f68093d131c7fd961564458401b07f8c87ff8f6030c1a0c7301".into(),
        },
        Vector {
            language: Language::French,
            seed: "poids vaseux tarte bazar poivre effet entier nuance \
               sensuel ennui pacte osselet poudre battre alibi mouton \
               stade paquet pliage gibier type question position projet pliage"
                .into(),
            spend: "2dd39ff1a4628a94b5c2ec3e42fb3dfe15c2b2f010154dc3b3de6791e805b904".into(),
            view: "6725b32230400a1032f31d622b44c3a227f88258939b14a7c72e00939e7bdf0e".into(),
        },
        Vector {
            language: Language::Spanish,
            seed: "minero ocupar mirar evadir octubre cal logro miope \
               opaco disco ancla litio clase cuello nasal clase \
               fiar avance deseo mente grumo negro cordón croqueta clase"
                .into(),
            spend: "ae2c9bebdddac067d73ec0180147fc92bdf9ac7337f1bcafbbe57dd13558eb02".into(),
            view: "18deafb34d55b7a43cae2c1c1c206a3c80c12cc9d1f84640b484b95b7fec3e05".into(),
        },
        Vector {
            language: Language::German,
            seed: "Kaliber Gabelung Tapir Liveband Favorit Specht Enklave Nabel \
               Jupiter Foliant Chronik nisten löten Vase Aussage Rekord \
               Yeti Gesetz Eleganz Alraune Künstler Almweide Jahr Kastanie Almweide"
                .into(),
            spend: "79801b7a1b9796856e2397d862a113862e1fdc289a205e79d8d70995b276db06".into(),
            view: "99f0ec556643bd9c038a4ed86edcb9c6c16032c4622ed2e000299d527a792701".into(),
        },
        Vector {
            language: Language::Italian,
            seed: "cavo pancetta auto fulmine alleanza filmato diavolo prato \
               forzare meritare litigare lezione segreto evasione votare buio \
               licenza cliente dorso natale crescere vento tutelare vetta evasione"
                .into(),
            spend: "5e7fd774eb00fa5877e2a8b4dc9c7ffe111008a3891220b56a6e49ac816d650a".into(),
            view: "698a1dce6018aef5516e82ca0cb3e3ec7778d17dfb41a137567bfa2e55e63a03".into(),
        },
        Vector {
            language: Language::Portuguese,
            seed: "agito eventualidade onus itrio holograma sodomizar objetos dobro \
               iugoslavo bcrepuscular odalisca abjeto iuane darwinista eczema acetona \
               cibernetico hoquei gleba driver buffer azoto megera nogueira agito"
                .into(),
            spend: "13b3115f37e35c6aa1db97428b897e584698670c1b27854568d678e729200c0f".into(),
            view: "ad1b4fd35270f5f36c4da7166672b347e75c3f4d41346ec2a06d1d0193632801".into(),
        },
        Vector {
            language: Language::Japanese,
            seed: "ぜんぶ どうぐ おたがい せんきょ おうじ そんちょう じゅしん いろえんぴつ \
               かほう つかれる えらぶ にちじょう くのう にちようび ぬまえび さんきゃく \
               おおや ちぬき うすめる いがく せつでん さうな すいえい せつだん おおや"
                .into(),
            spend: "c56e895cdb13007eda8399222974cdbab493640663804b93cbef3d8c3df80b0b".into(),
            view: "6c3634a313ec2ee979d565c33888fd7c3502d696ce0134a8bc1a2698c7f2c508".into(),
        },
        Vector {
            language: Language::Russian,
            seed: "шатер икра нация ехать получать инерция доза реальный \
               рыжий таможня лопата душа веселый клетка атлас лекция \
               обгонять паек наивный лыжный дурак стать ежик задача паек"
                .into(),
            spend: "7cb5492df5eb2db4c84af20766391cd3e3662ab1a241c70fc881f3d02c381f05".into(),
            view: "fcd53e41ec0df995ab43927f7c44bc3359c93523d5009fb3f5ba87431d545a03".into(),
        },
        Vector {
            language: Language::Esperanto,
            seed: "ukazo klini peco etikedo fabriko imitado onklino urino \
               pudro incidento kumuluso ikono smirgi hirundo uretro krii \
               sparkado super speciala pupo alpinisto cvana vokegi zombio fabriko"
                .into(),
            spend: "82ebf0336d3b152701964ed41df6b6e9a035e57fc98b84039ed0bd4611c58904".into(),
            view: "cd4d120e1ea34360af528f6a3e6156063312d9cefc9aa6b5218d366c0ed6a201".into(),
        },
        Vector {
            language: Language::Lojban,
            seed: "jetnu vensa julne xrotu xamsi julne cutci dakli \
               mlatu xedja muvgau palpi xindo sfubu ciste cinri \
               blabi darno dembi janli blabi fenki bukpu burcu blabi"
                .into(),
            spend: "e4f8c6819ab6cf792cebb858caabac9307fd646901d72123e0367ebc0a79c200".into(),
            view: "c806ce62bafaa7b2d597f1a1e2dbe4a2f96bfd804bf6f8420fc7f4a6bd700c00".into(),
        },
        Vector {
            language: Language::DeprecatedEnglish,
            seed: "glorious especially puff son moment add youth nowhere \
               throw glide grip wrong rhythm consume very swear \
               bitter heavy eventually begin reason flirt type unable"
                .into(),
            spend: "647f4765b66b636ff07170ab6280a9a6804dfbaf19db2ad37d23be024a18730b".into(),
            view: "045da65316a906a8c30046053119c18020b07a7a3a6ef5c01ab2a8755416bd02".into(),
        },
        // The following seeds require the language specification in order to calculate
        // a single valid checksum
        Vector {
            language: Language::Spanish,
            seed: "pluma laico atraer pintor peor cerca balde buscar \
               lancha batir nulo reloj resto gemelo nevera poder columna gol \
               oveja latir amplio bolero feliz fuerza nevera"
                .into(),
            spend: "30303983fc8d215dd020cc6b8223793318d55c466a86e4390954f373fdc7200a".into(),
            view: "97c649143f3c147ba59aa5506cc09c7992c5c219bb26964442142bf97980800e".into(),
        },
        Vector {
            language: Language::Spanish,
            seed: "pluma pluma pluma pluma pluma pluma pluma pluma \
               pluma pluma pluma pluma pluma pluma pluma pluma \
               pluma pluma pluma pluma pluma pluma pluma pluma pluma"
                .into(),
            spend: "b4050000b4050000b4050000b4050000b4050000b4050000b4050000b4050000".into(),
            view: "d73534f7912b395eb70ef911791a2814eb6df7ce56528eaaa83ff2b72d9f5e0f".into(),
        },
        Vector {
            language: Language::English,
            seed: "plus plus plus plus plus plus plus plus \
               plus plus plus plus plus plus plus plus \
               plus plus plus plus plus plus plus plus plus"
                .into(),
            spend: "3b0400003b0400003b0400003b0400003b0400003b0400003b0400003b040000".into(),
            view: "43a8a7715eed11eff145a2024ddcc39740255156da7bbd736ee66a0838053a02".into(),
        },
        Vector {
            language: Language::Spanish,
            seed: "audio audio audio audio audio audio audio audio \
               audio audio audio audio audio audio audio audio \
               audio audio audio audio audio audio audio audio audio"
                .into(),
            spend: "ba000000ba000000ba000000ba000000ba000000ba000000ba000000ba000000".into(),
            view: "1437256da2c85d029b293d8c6b1d625d9374969301869b12f37186e3f906c708".into(),
        },
        Vector {
            language: Language::English,
            seed: "audio audio audio audio audio audio audio audio \
               audio audio audio audio audio audio audio audio \
               audio audio audio audio audio audio audio audio audio"
                .into(),
            spend: "7900000079000000790000007900000079000000790000007900000079000000".into(),
            view: "20bec797ab96780ae6a045dd816676ca7ed1d7c6773f7022d03ad234b581d600".into(),
        },
    ];

    for vector in vectors {
        fn trim_by_lang(word: &str, lang: Language) -> String {
            if lang != Language::DeprecatedEnglish {
                word.chars()
                    .take(LANGUAGES[&lang].unique_prefix_length)
                    .collect()
            } else {
                word.to_string()
            }
        }

        let trim_seed = |seed: &str| {
            seed.split_whitespace()
                .map(|word| trim_by_lang(word, vector.language))
                .collect::<Vec<_>>()
                .join(" ")
        };

        // Test against Monero
        {
            println!(
                "{}. language: {:?}, seed: {}",
                line!(),
                vector.language,
                vector.seed.clone()
            );
            let seed =
                Seed::from_string(vector.language, Zeroizing::new(vector.seed.clone())).unwrap();
            let trim = trim_seed(&vector.seed);
            assert_eq!(
                seed,
                Seed::from_string(vector.language, Zeroizing::new(trim)).unwrap()
            );

            let spend: [u8; 32] = hex::decode(vector.spend).unwrap().try_into().unwrap();
            // For originalal seeds, Monero directly uses the entropy as a spend key
            assert_eq!(
                Option::<Scalar>::from(Scalar::from_canonical_bytes(*seed.entropy())),
                Option::<Scalar>::from(Scalar::from_canonical_bytes(spend)),
            );

            let view: [u8; 32] = hex::decode(vector.view).unwrap().try_into().unwrap();
            // Monero then derives the view key as H(spend)
            assert_eq!(
                Scalar::from_bytes_mod_order(keccak256(spend)),
                Scalar::from_canonical_bytes(view).unwrap()
            );

            assert_eq!(
                Seed::from_entropy(vector.language, Zeroizing::new(spend)).unwrap(),
                seed
            );
        }

        // Test against ourselves
        {
            let seed = Seed::new(&mut OsRng, vector.language);
            println!("{}. seed: {}", line!(), *seed.to_string());
            let trim = trim_seed(&seed.to_string());
            assert_eq!(
                seed,
                Seed::from_string(vector.language, Zeroizing::new(trim)).unwrap()
            );
            assert_eq!(
                seed,
                Seed::from_entropy(vector.language, seed.entropy()).unwrap()
            );
            assert_eq!(
                seed,
                Seed::from_string(vector.language, seed.to_string()).unwrap()
            );
        }
    }
}
