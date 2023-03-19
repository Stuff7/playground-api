use super::{Collection, DBResult, File, PartialFile};
use format as f;
use mongodb::bson::{doc, to_bson, to_document, Document};
use std::collections::HashSet;

pub(super) fn query_all_children() -> Document {
  doc! { "$graphLookup": {
    "from": File::collection_name(),
    "startWith": "$_id",
    "connectFromField": "_id",
    "connectToField": File::folder_id(),
    "as": "children",
    "maxDepth": 99,
  } }
}

pub(super) fn query_all_parents() -> Document {
  doc! { "$graphLookup": {
    "from": File::collection_name(),
    "startWith": f!("${}", File::folder_id()),
    "connectFromField": File::folder_id(),
    "connectToField": "_id",
    "as": "parents",
    "maxDepth": 99,
    "restrictSearchWithMatch": { "metadata.type": "folder" }
  } }
}

pub(super) fn query_direct_children() -> Document {
  doc! { "$lookup": {
    "from": File::collection_name(),
    "pipeline": [
      { "$addFields": {
        "insensitiveName": { "$toLower": f!("${}", File::name()) },
      } },
      { "$sort": { "insensitiveName": 1 } },
      { "$project": { "insensitiveName": 0 } }
    ],
    "localField": "_id",
    "foreignField": File::folder_id(),
    "as": "directChildren",
  } }
}

pub(super) fn query_by_file(file: &PartialFile) -> DBResult<Document> {
  Ok(to_document::<PartialFile>(file)?)
}

pub(super) fn query_by_id(user_id: &str, id: &str) -> DBResult<Document> {
  Ok(doc! { File::user_id(): user_id, "_id": File::map_folder_id(user_id, id) })
}

pub(super) fn query_many_by_id(
  user_id: &str,
  ids: &HashSet<String>,
) -> DBResult<Document> {
  Ok(
    doc! { File::user_id(): user_id, "_id": { "$in": to_bson::<HashSet<String>>(ids)? } },
  )
}
