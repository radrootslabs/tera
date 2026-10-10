extension TeraLocalNetwork: CustomDebugStringConvertible {
  var debugDescription: String {
    "TeraLocalNetwork(schemaVersion: \(schemaVersion), relayCount: \(relayURLs.count), followedAuthorCount: \(followedAuthors.count), generation: \(generation))"
  }
}
