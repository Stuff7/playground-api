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

## **Get current user info**

```
GET /api/users/me
```

#### **Response**

```json
{
  "_id": "google@user1",
  "picture": "profile-pic-url",
  "linked_accounts": [
    "google@user1",
    "google@user2"
  ]
}
```

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
      <td>Get only the files belonging to this provider ID</td>
    </tr>
  </tbody>
</table>

#### **Response**

```json
{
  "google@user1": [
    {
      "kind": "drive#file",
      "id": "some-id",
      "mime_type": "video/quicktime",
      "name": "video.MOV",
      "size": "3355843957",
      "video_metadata": {
        "width": 1920,
        "height": 1080,
        "duration_millis": "1725298"
      }
    },
    ...
  ],
  "google@user2": ...
}
```

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

```json
{
  "kind": "drive#file",
  "id": "some-id",
  "mime_type": "video/quicktime",
  "name": "video.MOV",
  "size": "3355843957",
  "video_metadata": {
    "width": 1920,
    "height": 1080,
    "duration_millis": "1725298"
  }
}
```

</br>

---

## **Stream drive video**

```
GET /drive/video/:video_id
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
