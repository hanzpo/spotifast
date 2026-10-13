//! Spotify's own GraphQL service, "pathfinder", as its desktop app asks
//! it: the app's pages (search, artists, Home, profiles) come from here
//! rather than the Web API, under the session's own token, so they share
//! no app's quota.
//!
//! Each query is persisted on Spotify's side and named by its hash. The
//! hashes are the desktop app's (Spotify 1.3.3.264, read from its
//! `xpui.spa`) and, for the queries its core keeps compiled out of reach,
//! the web player's, which shares the rest of them. Spotify replaces them
//! as its apps change, and a query whose hash has gone fails, for the
//! caller to fall back on the Web API.

use anyhow::{Context as _, anyhow};
use bytes::Bytes;
use http::{Method, Request, header};
use librespot_core::Session;
use serde_json::{Value, json};

use crate::api::models::{Artist, Image};

const ENDPOINT: &str = "https://api-partner.spotify.com/pathfinder/v2/query";

/// The desktop app's queries, by name, with their hashes.
const QUERIES: &[(&str, &str)] = &[
    (
        "ArtistConcerts",
        "ef53c43b865496b9890b7167eab1dc614a8949ef9451b3c41184ea888de8bd2b",
    ),
    (
        "ArtistConcertsPageLocation",
        "320698465a352f0d0247ec8ed02471244106d4199820f99de4d0a785561c2b03",
    ),
    (
        "ConcertCampaign",
        "c7d2e205f8e9a86ff591b04dec97785769b403f8b936a0826c936d2ea0c4add9",
    ),
    (
        "ConcertGallery",
        "2712501d317e8fb585b74dfdbaeaf4f36f9d7d49fc8620b29e307b2fe0f053f7",
    ),
    (
        "accountAttributes",
        "d48062d0df0a256ff78ed9e232b83d238dcd710b371efd304b0e178b62595582",
    ),
    (
        "albumPreRelease",
        "e1a5537a42c3b8a687f10ae865c3b6776d6d797f8c0c272b2d40dc159a8e1a94",
    ),
    (
        "albumPreReleaseTracks",
        "dfbdcf2688995adc2c2196fcdd7802b2a5137a2549b361aa7fb23cd6493f4672",
    ),
    (
        "areEntitiesInLibrary",
        "134337999233cc6fdd6b1e6dbf94841409f04a946c5c7b744b09ba0dfe5a85ed",
    ),
    (
        "assistedCurationSearch",
        "f78953bf9207d73493c27284103f5aeb6e728876d5793851bf79bc706127ff70",
    ),
    (
        "assistedCurationSearchAlbum",
        "e33489c81fdab1986d8b785fb9bf13993a2d5ff171190c575963f97e525870fe",
    ),
    (
        "assistedCurationSearchArtist",
        "a562324cea8976b4d51f08cee971bb180ce9279c947d27e0814675220b160cf7",
    ),
    (
        "browseAll",
        "dbd8b55e09a58afc52eab438bc228ba28fd72ac2f2148c6c26354980e4579001",
    ),
    (
        "browsePage",
        "f5c4e6d668f5716464a231c1cc8b22c1cbf6ad68b09929fd7de813a30581298b",
    ),
    (
        "browseSection",
        "b13c1cccbfcb6947753c2613411b3566485c21fd5f36d80a80bb64be61ba2d51",
    ),
    (
        "canvas",
        "575138ab27cd5c1b3e54da54d0a7cc8d85485402de26340c2145f0f6bb5e7a9f",
    ),
    (
        "centralisedStatePlayerOptions",
        "e2dcfcab470854d4d1c7cb1a851438f14fe0a94d57db7f0b9dde492559d5395d",
    ),
    (
        "concert",
        "9b582a69061f6648858515c2aa6d4a2273ac49b74eb5dbc612bc1299a9f2f13e",
    ),
    (
        "concertConcepts",
        "a409c1eb39b6345e7993d424d2408b65a6699bafc2b8a03217033e517cd76b72",
    ),
    (
        "concertCount",
        "29be9d486e073a49268e13ed9e2d2180187e669fcb7a19b98011aca7ab61b141",
    ),
    (
        "concertFeed",
        "9cae2dbee3f47904c60bab45256260b3ddb9844d5ef25038c17112619d14ce9a",
    ),
    (
        "concertLocationDetails",
        "b13f195349f188fee25480ae889d782852d68663bf07743c654244454750d681",
    ),
    (
        "concertLocationsByLatLon",
        "8a059d072a17a1199feb21fe846271f1680eda87010c832852ced0c55c6c7c96",
    ),
    (
        "concertRoutingCardVisuals",
        "a69f1a97f02c81533bd4e4ac2fed4669cb35a44a294e36a02c14df892d63ff0f",
    ),
    (
        "countryHubContent",
        "0e82bf24f321731e43423d9cdce71aa154ee876cd12b055e7c3af4dcb44c9225",
    ),
    (
        "countryHubsPage",
        "6c2e4b04d8836507c2ad09c954f27a6c98d8b0a761f99ef6f4dc9bbe7834ba55",
    ),
    (
        "decorateContextEpisodesOrChapters",
        "383de00240775c39a6afe0b1055dc562b2a3930894201f9762f3fc32a74971c7",
    ),
    (
        "decorateContextTracks",
        "383de00240775c39a6afe0b1055dc562b2a3930894201f9762f3fc32a74971c7",
    ),
    (
        "decorateQueuedByUsers",
        "33fe7085b4a9dde404955eeee4f8f93aa7a8e55028058354bc707727d8a7890d",
    ),
    (
        "editablePlaylists",
        "d5c4b8096437dcc2ac9528c91dfcd299e35b747cda2f8f75d28f41f49c5092ba",
    ),
    (
        "episodeSponsoredContent",
        "a5c1fe722b60c29ad247ea3df57ace52043382a7f080d525f58745db78a42618",
    ),
    (
        "feedBaselineLookup",
        "318190494c436b51ab995367bffc66dcdb49dfa023792c60ac15dcb6dfbec42f",
    ),
    (
        "fetchEntitiesForRecentlyPlayed",
        "cf5d2e94ffd82788470788ae1f6090cc3e9e774fb8fd383580634c6e6f50f7be",
    ),
    (
        "fetchExtractedColorAndImageForAlbumEntity",
        "b2d6d99fb6237952dfa6638385f9c6085f1b1fb9d0468753ca6ad98ff45adc6f",
    ),
    (
        "fetchExtractedColorAndImageForArtistEntity",
        "b2d6d99fb6237952dfa6638385f9c6085f1b1fb9d0468753ca6ad98ff45adc6f",
    ),
    (
        "fetchExtractedColorAndImageForEpisodeEntity",
        "b2d6d99fb6237952dfa6638385f9c6085f1b1fb9d0468753ca6ad98ff45adc6f",
    ),
    (
        "fetchExtractedColorAndImageForPlaylistEntity",
        "b2d6d99fb6237952dfa6638385f9c6085f1b1fb9d0468753ca6ad98ff45adc6f",
    ),
    (
        "fetchExtractedColorAndImageForPodcastEntity",
        "b2d6d99fb6237952dfa6638385f9c6085f1b1fb9d0468753ca6ad98ff45adc6f",
    ),
    (
        "fetchExtractedColorAndImageForTrackEntity",
        "b2d6d99fb6237952dfa6638385f9c6085f1b1fb9d0468753ca6ad98ff45adc6f",
    ),
    (
        "fetchExtractedColorForAlbumEntity",
        "b2d6d99fb6237952dfa6638385f9c6085f1b1fb9d0468753ca6ad98ff45adc6f",
    ),
    (
        "fetchExtractedColorForArtistEntity",
        "b2d6d99fb6237952dfa6638385f9c6085f1b1fb9d0468753ca6ad98ff45adc6f",
    ),
    (
        "fetchExtractedColorForEpisodeEntity",
        "b2d6d99fb6237952dfa6638385f9c6085f1b1fb9d0468753ca6ad98ff45adc6f",
    ),
    (
        "fetchExtractedColorForPlaylistEntity",
        "b2d6d99fb6237952dfa6638385f9c6085f1b1fb9d0468753ca6ad98ff45adc6f",
    ),
    (
        "fetchExtractedColorForPodcastEntity",
        "b2d6d99fb6237952dfa6638385f9c6085f1b1fb9d0468753ca6ad98ff45adc6f",
    ),
    (
        "fetchExtractedColorForTrackEntity",
        "b2d6d99fb6237952dfa6638385f9c6085f1b1fb9d0468753ca6ad98ff45adc6f",
    ),
    (
        "fetchExtractedColors",
        "36e90fcaea00d47c695fce31874efeb2519b97d4cd0ee1abfb4f8dc9348596ea",
    ),
    (
        "fetchLibraryTracks",
        "087278b20b743578a6262c2b0b4bcd20d879c503cc359a2285baf083ef944240",
    ),
    (
        "fetchPlaylist",
        "8964e8eafb21aa992a7d951d256d83285c04be2105d209262901de70cb97584a",
    ),
    (
        "fetchPlaylistContents",
        "8964e8eafb21aa992a7d951d256d83285c04be2105d209262901de70cb97584a",
    ),
    (
        "fetchPlaylistMetadata",
        "8964e8eafb21aa992a7d951d256d83285c04be2105d209262901de70cb97584a",
    ),
    (
        "getAlbum",
        "11c9cad9cb24c4f9e25ef9456f68621cfd6006a90f679e729c1d2f2fd70dfe16",
    ),
    (
        "getAlbumNameAndTracks",
        "8628ad33de3267d7bef516c76a746979a5f98891a2c9eaff3dfec828abdcd983",
    ),
    (
        "getArtistNameAndTracks",
        "0adaf1a1a8a94c7ed095639c4d9456d2b1cfac16ac511d5dd2b01b6dd89f748a",
    ),
    (
        "getAudiobooksMetadata",
        "523c71c64749a628f83e6b31a122c76663243730bf02df01fd64abf0f62f572f",
    ),
    (
        "getCommentsForEntity",
        "5075cb710c32d369eb5b67f6abdfd11a48d58214795064181a7da830b5fc84b6",
    ),
    (
        "getDynamicColors",
        "f0f112945d6d745bd8ff790317bbf8d310036da75df33130490e9d6dc96c59d9",
    ),
    (
        "getDynamicColorsByUris",
        "f0f112945d6d745bd8ff790317bbf8d310036da75df33130490e9d6dc96c59d9",
    ),
    (
        "getEpisodeName",
        "508f9db2e7dc340c338950dc67a6045ee1406703646f23b760986fa689c239b1",
    ),
    (
        "getEpisodeOrChapter",
        "5f77db47e5a2ce6680330bb61317aa09e17ddd1d189513cb218e7cd3e43288be",
    ),
    (
        "getLists",
        "8dbc24274768a082af07ff1b2a1a891c863b26ab5c64c256b56f3ceb9d208a1a",
    ),
    (
        "getListsContents",
        "8dbc24274768a082af07ff1b2a1a891c863b26ab5c64c256b56f3ceb9d208a1a",
    ),
    (
        "getListsMetadata",
        "8dbc24274768a082af07ff1b2a1a891c863b26ab5c64c256b56f3ceb9d208a1a",
    ),
    (
        "getPodcastOrBookName",
        "631676b4cf1eb7c93d1133e3f1f17e5bfe8d6a5e2fb9560148bac61f1531f267",
    ),
    (
        "getReactions",
        "0d209bf9507779887fe2b3032d1afd8f35de8425b01aead094698ff1abecda71",
    ),
    (
        "getReplies",
        "4c45f55fdfd691e08ad983d5c914baf5581824a81b2da1cdb8a9dad747c012bf",
    ),
    (
        "getTrack",
        "a8ef9e9f02b836feb0da3003c31dbb30decc6f4b473ef89ca88c882386d668de",
    ),
    (
        "getTrackName",
        "3dee761788854e8dd9239e13ce0d712da031fb8c2036f096a1c765062b410660",
    ),
    (
        "getVideoTrackAssociatedAlbum",
        "e9ecdc49f7777062fc841415262d47c3927e09ab6e6845b420a373caec602812",
    ),
    (
        "home",
        "5fb7da7a03ee2776c0670856609cb85a189a3cecc36cc160b91cde79063b9cf1",
    ),
    (
        "homePinnedSections",
        "5fb7da7a03ee2776c0670856609cb85a189a3cecc36cc160b91cde79063b9cf1",
    ),
    (
        "homeSection",
        "5fb7da7a03ee2776c0670856609cb85a189a3cecc36cc160b91cde79063b9cf1",
    ),
    (
        "inferredUserLocation",
        "5db4c507ea735d2a1f37bd1166eca2c1a0e3387bb875ebca5d6031b6eccceeba",
    ),
    (
        "internalLinkRecommenderEpisode",
        "122f5c777aae5c0918baec11cd646b7034b8f213f260097b4d229ad947ec7f93",
    ),
    (
        "internalLinkRecommenderShow",
        "6c369ff272a666b31fef1629c169925a1bd80f372195396c82304142cacd89e8",
    ),
    (
        "internalLinkRecommenderTrack",
        "c77098ee9d6ee8ad3eb844938722db60570d040b49f41f5ec6e7be9160a7c86b",
    ),
    (
        "isCurated",
        "e4ed1f91a2cc5415befedb85acf8671dc1a4bf3ca1a5b945a6386101a22e28a6",
    ),
    (
        "isCuratedEntities",
        "af6bb0d2691f78f9169e1ba2dfed34a414bb4994e858f81487d6a26b95280566",
    ),
    (
        "isFollowingUsers",
        "c00e0cb6c7766e7230fc256cf4fe07aec63b53d1160a323940fce7b664e95596",
    ),
    (
        "libraryV3",
        "390c78e5b951029bad359785e69b07b536a509c581cbcd0aded5e5067f187455",
    ),
    (
        "lookupChildEntities",
        "91ce02e32b19123de231dc8de91fe4b9ab84eca087d4c015549308d77fbb6d10",
    ),
    (
        "lookupDebug",
        "6953237e41493792c4e838ebe9532a4cad6a6c11d77229fb524a5e966b855fef",
    ),
    (
        "lookupEntityImages",
        "35e3dfa923e93cd4840a8f7847058d08d0f5a7862c158644a32a59ca28b6122c",
    ),
    (
        "npvPageContent",
        "6e58365b05723c3f917b0df611cbd11dae31d36cc3e71a8dd73d72300b663b46",
    ),
    (
        "npvV2TrackSupplementalSections",
        "3268e307f08330b620de5c161a03b48acf66ec57164eac446e0653694a485764",
    ),
    (
        "playlistPermissions",
        "e43d1d35f231cf289c23c9d9c489f4a4f502e4eda09839c530608f107b6556b8",
    ),
    (
        "playlistSection",
        "2615df403a9043c1d7d3094fbeb4c9653b07b11a33d8081fbd31f0f7959ff4a1",
    ),
    (
        "profileAttributes",
        "08ffb4730af3746e04a8301396f20875dbbce10c75243803091a9274eacc8ac0",
    ),
    (
        "queryAlbumMerch",
        "3ef44ed6f17be67299538fe77faffab4075aeaf9e1085f10fc835592266711b5",
    ),
    (
        "queryAlbumTrackUris",
        "a2a17981f8439ca1798f56260277d9d7800ec0ca7040053b564e0f975d8aa344",
    ),
    (
        "queryAlbumTracks",
        "11c9cad9cb24c4f9e25ef9456f68621cfd6006a90f679e729c1d2f2fd70dfe16",
    ),
    (
        "queryArtistAboutModal",
        "702e29a866e0b4b4fabdf974454512a2c32d93643b1969b7a3a0dd92084f05f6",
    ),
    (
        "queryArtistAppearsOn",
        "9a4bb7a20d6720fe52d7b47bc001cfa91940ddf5e7113761460b4a288d18a4c1",
    ),
    (
        "queryArtistDiscographyAlbums",
        "5e07d323febb57b4a56a42abbf781490e58764aa45feb6e3dc0591564fc56599",
    ),
    (
        "queryArtistDiscographyAll",
        "5e07d323febb57b4a56a42abbf781490e58764aa45feb6e3dc0591564fc56599",
    ),
    (
        "queryArtistDiscographyCompilations",
        "5e07d323febb57b4a56a42abbf781490e58764aa45feb6e3dc0591564fc56599",
    ),
    (
        "queryArtistDiscographyOverview",
        "5e07d323febb57b4a56a42abbf781490e58764aa45feb6e3dc0591564fc56599",
    ),
    (
        "queryArtistDiscographySingles",
        "5e07d323febb57b4a56a42abbf781490e58764aa45feb6e3dc0591564fc56599",
    ),
    (
        "queryArtistDiscoveredOn",
        "71c2392e4cecf6b48b9ad1311ae08838cbdabcfd189c6bf0c66c2430b8dcfdb1",
    ),
    (
        "queryArtistFeaturing",
        "20842d6d9d2d28ef945984b68cb927bb33edd00eab84a8da1667def21f1f2c54",
    ),
    (
        "queryArtistMinimal",
        "53d3f76582c49ad0a05dc685955f20dc2a5f2209b192e5446e5e4e623ce23a48",
    ),
    (
        "queryArtistOverview",
        "1ac33ddab5d39a3a9c27802774e6d78b9405cc188c6f75aed007df2a32737c72",
    ),
    (
        "queryArtistPlaylists",
        "54f7e5a5a2af05b7dc98526df376a46c6b15c05440c8dfdc8f6cecb1a807eca7",
    ),
    (
        "queryArtistRelated",
        "3d031d6cb22a2aa7c8d203d49b49df731f58b1e2799cc38d9876d58771aa66f3",
    ),
    (
        "queryArtistRelatedVideos",
        "e4bbb4f420ad5786caa7028e2943542f13c8138f5c40736292b8499ac9672ce6",
    ),
    (
        "queryBookChapters",
        "8f342d1c624755901657fa65cbb80dd3bacbcca2f6d802f570ae3269d59a403e",
    ),
    (
        "queryInlineCurationSearchAlbum",
        "74a97c6afa1c6c23e1d9b897756fc362ca7e5ed718ac59ba65ffa85f687f7113",
    ),
    (
        "queryInlineCurationSearchArtist",
        "657c07a8943ea6e49c61833dde16d8d51cb64e921dd80827255d6e2519621c34",
    ),
    (
        "queryInlineCurationSearchV2",
        "857838f3d49dde22e8d7bead4487d4cdb662f8daad165b80c0cad270ae2ff37b",
    ),
    (
        "queryInlineCurationSearchV2Booklists",
        "857838f3d49dde22e8d7bead4487d4cdb662f8daad165b80c0cad270ae2ff37b",
    ),
    (
        "queryNpvArtist",
        "e1ae46a21911a3075c1aa29bf09a6c60f9a45b6a2b1132429f10ad06c299b5d7",
    ),
    (
        "queryNpvEpisode",
        "5704293e4a52b987c87bc11afe210aaca52813405c85ff8d0632e7587ebc6e18",
    ),
    (
        "queryPodcastEpisodes",
        "3539d746cf882f3909660de40b4ef472b3f5893a0761d617c282541de5d412c5",
    ),
    (
        "queryShowMetadataV2",
        "b475447846f37cc426add800b995a7859c169c53c72b62de6338c0c40a5dacea",
    ),
    (
        "queryTrackArtists",
        "ee2b038198f5e62c679c3996584d9249bbee55fe69fc212271c56492a022c798",
    ),
    (
        "queryTrackCreditsGroupedModal",
        "f135fb9be58a72d041ab5d214d817021a272405d883860468e2627afb01a3ca9",
    ),
    (
        "queryWhatsNewFeed",
        "d889c8c936ab192af8ced595427f5ba2acdf63478fdc0a181c8d477f8322630e",
    ),
    (
        "recentSearches",
        "ece96d7b82c820fb9938dd1df50dee3e53737d808c3883e3d01c467c423601b0",
    ),
    (
        "recents",
        "698be5892a3cc95331deebeff463d05dfdd5febf5254bea30b895b5a93dfb584",
    ),
    (
        "searchAlbums",
        "202cb3305e31e5a0767ba7925f28bd728cf8f8b0217e6da43909056071cd70e9",
    ),
    (
        "searchArtists",
        "7bf95d754fdbe32c8b161fbbe54d1ae50974900df4dce4c8f1afcbcad153224d",
    ),
    (
        "searchAudiobooks",
        "e05ac765d02c084f8783d3c1572b23d57761c43f47eb8b87ce2f9ccced3fa068",
    ),
    (
        "searchAuthors",
        "4a9d403a7cbc7e19da5520d619a865472b35382b043bfa458154e73a5c6f46bd",
    ),
    (
        "searchConcertLocations",
        "43ededefcba8b3f519fd0c2d6c025dfeec9f742cf47d04a3c3711d95b27deda3",
    ),
    (
        "searchDesktop",
        "1148393611bbc58e84e47aed35ecc731275df9f9eb660956962e352dd3631d89",
    ),
    (
        "searchEpisodes",
        "c9ee277c533bd3f191f9f09bee04f8e4e81bdc48f4d2fadbf67e504c75dc3fe1",
    ),
    (
        "searchFullEpisodes",
        "d54e35fafe7520cb53883b86d012911cbad75c14ac079a917951c24cdb07c60f",
    ),
    (
        "searchGenres",
        "9e1c0e056c46239dd1956ea915b988913c87c04ce3dadccdb537774490266f46",
    ),
    (
        "searchModalEntityPage",
        "25d73ce01f3011ff8b37f44c23ebbad18610db1a2cec2c84f20c1e822c748524",
    ),
    (
        "searchModalResults",
        "9a79751378a306d87d9789fa0c4a8f7c7213891c47a3993a0d0b58ff26514e59",
    ),
    (
        "searchPlaylists",
        "d520014e748f9ea44f7707d8df1819867ac1205e8b7f3e28f22fe5fc858921b1",
    ),
    (
        "searchPodcasts",
        "0195d9f61b43606d490bca64c3456e3593528cea6cc05c7e822c7c42beed0f4e",
    ),
    (
        "searchSuggestions",
        "f244254b94c0e824d458ac216e0f8406a73f2f69a9ff52831a2f78fa774ff6ce",
    ),
    (
        "searchTopResultsList",
        "12d940b3f7381506a82ff5382eb59b0ed65118656e6bcdfbe4cebc528f75d29d",
    ),
    (
        "searchTopResultsOnly",
        "aadb6a92e246cb76e3986777c161dec16085a08e998b420a5c2ce10aa52e4e11",
    ),
    (
        "searchTracks",
        "b02683192a98dde7966b5e6655a79eeb62713eab703eda9902c932818dd52751",
    ),
    (
        "searchUsers",
        "8f358dd82e62f61dd4ceaa9f8cd0889e644c9b707f1b724fbfb356a757cb7e5a",
    ),
    (
        "seoRecommendedTrackPlaylistDesktop",
        "2121830f81030dea46648d65a05c430cd822f08db6aaa18c7f35cc9b93c22396",
    ),
    (
        "showItemsPlayedState",
        "4a070b9bfab2e8537a5271e6839bc3ef51501dcac8170b5fb69a98f967c5fb60",
    ),
    (
        "similarAlbumsBasedOnThisTrack",
        "1d1f93a737498adca2c892c73af87fc0b052afe4e1a33c989540c32413dfae17",
    ),
    (
        "similarAudiobooks",
        "d62da1bbf5859e5a156da1f52ce21a73a642f925db717a179fd89626b27a80b8",
    ),
    (
        "smartShuffle",
        "3384085be84fbf2f855b024f99bc06cded1c0fd71af3a8fb8abb84e9656faba2",
    ),
    (
        "thisisPlaylistExtension",
        "906c2e14927400c8a71645c647576ce032f837a112d5e6f9375b19c80445c8cf",
    ),
    (
        "trackPreview",
        "fc26ffc7a1a4f93bd4c2d705649f7dba1de34005b3dc2915549847a9959405d8",
    ),
    (
        "userAccountId",
        "c56c2b33c2ded49960530771844ed482ffc1518169fb3d0dda91ac1608b019e2",
    ),
    (
        "userLocation",
        "079939378ca79b67c6d047be9152ea940d21f10bbfa2f5d4cf4d8320d87774c2",
    ),
    (
        "userTopContent",
        "49ee15704de4a7fdeac65a02db20604aa11e46f02e809c55d9a89f6db9754356",
    ),
    (
        "venue",
        "6c55ebc91fc13a25d7df590c4a9031dd1dcdc25716192d7f848a8270f49d2c13",
    ),
    (
        "watchFeedEntity",
        "d1a805f815ee0c20281330317d8c00c8fce9026686a86c38bc700ccaefb9ac5c",
    ),
    (
        "watchFeedView",
        "ce10864fccc53ca77c422143ee027bacf220f9d714e27ec592905334079ad743",
    ),
    (
        "whatsNewFeedNewItems",
        "d889c8c936ab192af8ced595427f5ba2acdf63478fdc0a181c8d477f8322630e",
    ),
];

fn hash(operation: &str) -> Option<&'static str> {
    QUERIES
        .iter()
        .find(|(name, _)| *name == operation)
        .map(|(_, hash)| *hash)
}

/// Asks pathfinder one of the desktop app's queries, and gives its `data`.
pub async fn query(session: &Session, operation: &str, variables: Value) -> anyhow::Result<Value> {
    let hash = hash(operation).ok_or_else(|| anyhow!("no pathfinder query named {operation}"))?;
    let body = json!({
        "variables": variables,
        "operationName": operation,
        "extensions": { "persistedQuery": { "version": 1, "sha256Hash": hash } },
    });
    let token = session.login5().auth_token().await?;
    let mut request = Request::builder()
        .method(Method::POST)
        .uri(ENDPOINT)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "application/json")
        .header(
            header::AUTHORIZATION,
            format!("{} {}", token.token_type, token.access_token),
        )
        .header("app-platform", "OSX_ARM64")
        .body(Bytes::from(serde_json::to_vec(&body)?))?;
    if let Ok(client_token) = session.spclient().client_token().await {
        request
            .headers_mut()
            .insert("client-token", http::HeaderValue::from_str(&client_token)?);
    }
    let bytes = session
        .http_client()
        .request_body(request)
        .await
        .with_context(|| format!("pathfinder {operation}"))?;
    let mut answer: Value = serde_json::from_slice(&bytes)?;
    if let Some(errors) = answer.get("errors").filter(|errors| !errors.is_null()) {
        if answer.get("data").is_none_or(Value::is_null) {
            return Err(anyhow!("pathfinder {operation}: {errors}"));
        }
        log::debug!("pathfinder {operation} answered with errors: {errors}");
    }
    Ok(answer["data"].take())
}

/// The pictures in a pathfinder image source list, as the Web API gives
/// them.
fn images(sources: &Value) -> Vec<Image> {
    sources
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|source| {
            Some(Image {
                url: source["url"].as_str()?.to_string(),
                width: source["width"].as_u64().map(|width| width as u32),
                height: source["height"].as_u64().map(|height| height as u32),
            })
        })
        .collect()
}

/// An artist as pathfinder names one, with its picture.
fn artist(data: &Value) -> Option<Artist> {
    let uri = data["uri"].as_str()?;
    Some(Artist {
        id: uri.strip_prefix("spotify:artist:")?.to_string(),
        uri: uri.to_string(),
        name: data["profile"]["name"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        images: images(&data["visuals"]["avatarImage"]["sources"]),
        ..Default::default()
    })
}

/// The artists Spotify relates to one, as its artist page lists them.
pub async fn related_artists(session: &Session, id: &str) -> anyhow::Result<Vec<Artist>> {
    let data = query(
        session,
        "queryArtistRelated",
        json!({ "uri": format!("spotify:artist:{id}") }),
    )
    .await?;
    Ok(related(&data))
}

fn related(data: &Value) -> Vec<Artist> {
    data["artistUnion"]["relatedContent"]["relatedArtists"]["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(artist)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn related_artists_keep_their_names_and_pictures() {
        let data = json!({ "artistUnion": { "relatedContent": { "relatedArtists": { "items": [
            {
                "id": "0XOT5JvyItELtax6yuzvWl",
                "uri": "spotify:artist:0XOT5JvyItELtax6yuzvWl",
                "profile": { "name": "kuala" },
                "visuals": { "avatarImage": { "sources": [
                    { "url": "https://i.scdn.co/image/a", "width": 640, "height": 640 }
                ] } }
            },
            { "profile": { "name": "no uri" } }
        ] } } } });
        let artists = related(&data);
        assert_eq!(artists.len(), 1);
        assert_eq!(artists[0].id, "0XOT5JvyItELtax6yuzvWl");
        assert_eq!(artists[0].name, "kuala");
        assert_eq!(artists[0].images[0].width, Some(640));
    }

    #[test]
    fn every_query_has_one_full_hash() {
        let mut names: Vec<_> = QUERIES.iter().map(|(name, _)| *name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), QUERIES.len());
        assert!(
            QUERIES
                .iter()
                .all(|(_, hash)| hash.len() == 64
                    && hash.bytes().all(|byte| byte.is_ascii_hexdigit()))
        );
        assert!(hash("searchDesktop").is_some());
        assert!(hash("queryArtistRelated").is_some());
    }
}
