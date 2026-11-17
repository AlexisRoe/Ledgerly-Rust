# Swagger UI assets (served in the `local` profile only)

- `index.html`: copied unchanged from Ledgerly-Java `src/main/resources/swagger/index.html` at the commit in
  `vendor/ledgerly-java/SOURCE`.
- `swagger-ui-bundle.js`, `swagger-ui.css`, `LICENSE`, `NOTICE`, `swagger-ui-bundle.js.LICENSE.txt`: from the npm
  package `swagger-ui-dist@5.34.1` (Apache-2.0), the version of the `org.webjars:swagger-ui` webjar Java serves.
  Tarball integrity (npm registry): `sha512-VSr8fXfI93obYaQicLJbLUKGRxDojj/SfRJDDLCioGTUBj4BK/Nj4eE9jCeCAnJxVLi7iS+08PbiZLyrQKBkJQ==`.

They are embedded into the binary with `include_bytes!`; nothing is fetched at run time. Update them together with
Java's webjar version.
