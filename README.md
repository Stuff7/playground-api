# **Playground API**

As of right now the only feature in this API is to play private videos from google drive using the most simple google oauth system.

## **Required env vars**

```
ALLOWED_ORIGINS
GOOGLE_CLIENT_ID
GOOGLE_CLIENT_SECRET
GOOGLE_REDIRECT_URL
JWT_SECRET
LOGIN_REDIRECT
MONGODB_URI
SOCKET_ADDRESS
```

# **Models**

<span id="ProviderID">

```typescript
type ProviderID = `${string}@${string}`;
```

</span>

<span id="User">

```typescript
interface User {
  _id: ProviderID,
  picture: string, // URL to user's profile picture 
  linkedAccounts: ProviderID[],
}
```

</span>

<span id="GoogleDriveKind">

```typescript
type GoogleDriveKind = `drive#${string}`;
```

</span>

<span id="GoogleDriveFile">

```typescript
interface GoogleDriveFile {
  kind: GoogleDriveKind,
  id: string,
  mimeType: string,
  name: string,
  size: `${number}`,
  videoMetadata: {
    "width": number,
    "height": number,
    "durationMillis": `${number}`,
  },
}
```

</span>

<span id="GoogleDriveFilesResponse">

```typescript
type GoogleDriveFilesResponse = Record<ProviderID, GoogleDriveFile[]>;
```

</span>

# **Routes**

## **Log in**

```
GET /api/google/login
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
      <td>current_login</td>
      <td>string</td>
      <td>Link the provider used to sign in to this token's user instead of creating a new user.</td>
    </tr>
  </tbody>
</table>

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

## **Get drive files**

```
GET /api/google/drive/files
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
      <td>user</td>
      <td>string</td>
      <td>Get only the files belonging to this provider ID.</td>
    </tr>
  </tbody>
</table>

#### **Response**

[`GoogleDriveFile`](#GoogleDriveFile) for every [`ProviderID`](#ProviderID) requested [`GoogleDriveFilesResponse`](#GoogleDriveFilesResponse).

</br>

---

## **Get single drive file**

```
GET /api/google/drive/files/:file_id
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
      <td>user</td>
      <td>string</td>
      <td>User to which this drive file belongs to (Defaults to logged in user).</td>
    </tr>
  </tbody>
</table>

#### **Response**

The [`GoogleDriveFile`](#GoogleDriveFile) requested .

</br>

---

## **Stream drive video**

```
GET /api/google/drive/video/:video_id
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
      <td>token (Required)</td>
      <td>string</td>
      <td>Authorization to access this file.</td>
    </tr>
    <tr>
      <td>user</td>
      <td>string</td>
      <td>User to which this drive file belongs to (Defaults to logged in user).</td>
    </tr>
  </tbody>
</table>

#### **Response**

Partial content video data (Intended to be used in html video element).
