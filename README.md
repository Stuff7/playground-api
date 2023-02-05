# **Playground API**

Google authentication and made up files system.

## **Required env vars**

```
SOCKET_ADDRESS
LOGIN_REDIRECT
ALLOWED_ORIGINS
JWT_SECRET
MONGODB_URI
GOOGLE_API_KEY
GOOGLE_CLIENT_ID
GOOGLE_CLIENT_SECRET
GOOGLE_REDIRECT_URL
```

# **Models**

<span id="UserID">

```typescript
type UserID = `${string}@${string}`;
```

</span>

<span id="User">

```typescript
interface User {
  _id: UserID,
  name: string,
  picture: string,
}
```

</span>

<span id="UserFile">

```typescript
interface UserFile {
  _id: string,
  folderId: string,
  userId: string,
  name: string,
  metadata: FileMetadata,
}
```

</span>

<span id="FileMetadata">

```typescript
type FileMetadata = Video | Folder;
```

</span>

<span id="Video">

```typescript
interface Video {
  type: "video",
  name: string,
  playId: string,
  durationMillis: number,
  width: number,
  height: number,
  thumbnail: string,
  mimeType: string,
  sizeBytes: number,
}
```

</span>

<span id="Folder">

```typescript
interface Folder {
  type: "folder",
}
```

</span>

# **Routes**

## **Log in**

```
GET /auth/google/login
```

#### **Response**

Redirects back to JSPlayground with the `access_token` as a query parameter

</br>

---

## **Log out**

```
DELETE /logout
```

#### **Response**

Invalidates session and returns no content 204

</br>

---

## **Ping**

```
GET /ping
```

#### **Response**

PONG

</br>

---

## **Get current user info**

```
GET /api/users/me
```

#### **Response**

The current logged in [`User`](#User).

</br>

---

## **List files**

*Requires Bearer Authorization*

```
GET /api/files
```

<table>
  <thead>
    <tr>
      <th>Parameter</th>
      <th>Value</th>
      <th>Description</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td>folder</td>
      <td>string</td>
      <td>Get files in this folder (Use "root" for top level folder).</td>
    </tr>
  </tbody>
</table>


#### **Response**

[`UserFile`](#UserFile) list

</br>

---

## **Update file**

*Requires Bearer Authorization*

```
PATCH /api/files/:file_id
```

<table>
  <thead>
    <tr>
      <th>Parameter</th>
      <th>Value</th>
      <th>Description</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td>folder</td>
      <td>string</td>
      <td>Move to this folder (Use "root" for top level folder).</td>
    </tr>
  </tbody>
</table>

**Request Body:** 

``` typescript
interface UpdateFileBody {
  name?: string,
}
```

#### **Response**

Updated [`UserFile`](#UserFile)

</br>

---

## **Delete file**

*Requires Bearer Authorization*

```
DELETE /api/files/:file_id
```

#### **Response**

The deleted [`UserFile`](#UserFile)

</br>

---

## **Create folder**

*Requires Bearer Authorization*

```
POST /api/files/folder
```

<table>
  <thead>
    <tr>
      <th>Parameter</th>
      <th>Value</th>
      <th>Description</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td>folder</td>
      <td>string</td>
      <td>Create folder inside this folder (Use "root" for top level folder).</td>
    </tr>
  </tbody>
</table>

**Request Body:** 

``` typescript
interface CreateFolderBody {
  name: string,
}
```

#### **Response**

Created [`UserFile`](#UserFile)

</br>

---

## **Move files to folder**

*Requires Bearer Authorization*

```
PUT /api/files/folder/move
```

<table>
  <thead>
    <tr>
      <th>Parameter</th>
      <th>Value</th>
      <th>Description</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td>folder</td>
      <td>string</td>
      <td>Move files to this folder (Use "root" for top level folder).</td>
    </tr>
  </tbody>
</table>

**Request Body:** 

``` typescript
interface MoveFilesBody {
  files: string[],
}
```

#### **Response**

``` typescript
interface MoveFilesResponse {
  movedCount: number,
}
```

</br>

---

## **Get video metadata**

```
GET /api/files/video/metadata
```
<table>
  <thead>
    <tr>
      <th>Parameter</th>
      <th>Value</th>
      <th>Description</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td>video_url</td>
      <td>string</td>
      <td>URL of the video (Only google drive supported).</td>
    </tr>
  </tbody>
</table>

#### **Response**

The [`Video`](#Video) metadata requested (`type` field is omitted).

</br>

---

## **Create video**

*Requires Bearer Authorization*

```
POST /api/files/video/:video_id
```

<table>
  <thead>
    <tr>
      <th>Parameter</th>
      <th>Value</th>
      <th>Description</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td>folder</td>
      <td>string</td>
      <td>Create video inside this folder (Use "root" for top level folder).</td>
    </tr>
  </tbody>
</table>

**Request Body:** 

``` typescript
interface CreateVideoBody {
  name?: string,
  thumbnail?: string,
}
```

#### **Response**

The created [`UserFile`](#UserFile)

</br>

---

## **Play video**

```
GET /api/files/video/:video_id
```

#### **Response**

Video content.
