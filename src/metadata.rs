use ttf_parser::{ Face, PlatformId, name_id };

fn get_name_string(face: &Face, target_name_id: u16) -> String {
    if let Some(name_table) = face.tables().name {
        let try_get = |platform_id, encoding_id| {
            name_table.names
                .into_iter()
                .find(|name_struct| {
                    name_struct.name_id == target_name_id &&
                        name_struct.platform_id == platform_id &&
                        name_struct.encoding_id == encoding_id
                })
        };

        if
            let Some(record) = try_get(PlatformId::Windows, 1).or_else(||
                try_get(PlatformId::Windows, 0)
            )
        {
            return record.to_string().unwrap_or_default();
        }
    }
    String::new()
}

pub fn get_font_family(face: &Face) -> String {
    let primary = get_name_string(face, name_id::TYPOGRAPHIC_FAMILY);
    if !primary.is_empty() {
        primary
    } else {
        get_name_string(face, name_id::FAMILY)
    }
}

pub fn get_font_subfamily(face: &Face) -> String {
    let primary = get_name_string(face, name_id::TYPOGRAPHIC_SUBFAMILY);
    if !primary.is_empty() {
        primary
    } else {
        get_name_string(face, name_id::SUBFAMILY)
    }
}

pub fn get_copyright_notice(face: &Face) -> String {
    get_name_string(face, name_id::COPYRIGHT_NOTICE)
}

pub fn get_license(face: &Face) -> String {
    get_name_string(face, name_id::LICENSE)
}

pub fn get_foundry(face: &Face) -> String {
    get_name_string(face, name_id::MANUFACTURER)
}

pub fn get_designer(face: &Face) -> String {
    get_name_string(face, name_id::DESIGNER)
}

pub fn calculate_checksum(data: &[u8]) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(data);
    format!("{}", hasher.finalize())
}

pub fn extract_metadata(
    data: &[u8]
) -> Result<Option<(String, String, String)>, Box<dyn std::error::Error>> {
    let checksum = calculate_checksum(data);

    let face = Face::parse(data, 0).map_err(|e| format!("Error parsing font data: {:?}", e))?;
    let family = get_font_family(&face);
    let subfamily = get_font_subfamily(&face);

    Ok(Some((family, subfamily, checksum)))
}
