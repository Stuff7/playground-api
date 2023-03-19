use super::{Collection, DBResult, File, PartialFile};
use format as f;
use mongodb::bson::{doc, to_bson, to_document, Document};
use std::collections::HashSet;

pub(super) fn query_lineage() -> Document {
  doc! { "$graphLookup": {
    "from": File::collection_name(),
    "startWith": "$_id",
    "connectFromField": "_id",
    "connectToField": File::folder_id(),
    "as": "lineage",
    "maxDepth": 99,
  } }
}

pub(super) fn query_ancestors() -> [Document; 5] {
  [
    doc! { "$graphLookup": {
      "from": File::collection_name(),
      "startWith": f!("${}", File::folder_id()),
      "connectFromField": File::folder_id(),
      "connectToField": "_id",
      "as": "ancestors",
      "maxDepth": 99,
      "restrictSearchWithMatch": { "metadata.type": "folder" },
      "depthField": "order"
    } },
    doc! { "$unwind": "$ancestors" },
    doc! { "$sort": { "ancestors.order": -1 } },
    doc! { "$group": {
      "_id": "$_id",
      "ancestors": { "$push": "$ancestors" },
      "root": { "$first": "$$ROOT" }
    } },
    doc! { "$replaceRoot": { "newRoot": {
      "$mergeObjects": [ "$root", { "ancestors": "$ancestors" } ]
    } } },
  ]
}

pub(super) fn query_children() -> Document {
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
    "as": "children",
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
