use super::*;

pub(crate) async fn exec(
    config: LairServerConfig,
    opt: OptImportSeed,
) -> LairResult<()> {
    // first start the server so the pid_file is acquired / etc
    let mut server =
        lair_keystore::server::StandaloneServer::new(config).await?;

    // then capture the needed passphrases
    let (store_pass, bundle_pass, deep_pass) = if opt.piped {
        let multi = read_piped_passphrase().await?;
        let multi = multi.read_lock();

        // careful not to move any bytes out of protected memory
        // convert to utf8 so we can use the rust split.
        let multi =
            std::str::from_utf8(&*multi).map_err(one_err::OneErr::new)?;
        let mut pass_list = multi
            .split('\n')
            .filter_map(|s| {
                let s = s.trim();
                let s = s.as_bytes();
                if s.is_empty() {
                    None
                } else {
                    let n = sodoken::BufWrite::new_mem_locked(s.len())
                        .expect("failed to allocate locked BufWrite");
                    {
                        let mut n = n.write_lock();
                        n.copy_from_slice(s);
                    }
                    Some(n.to_read())
                }
            })
            .collect::<Vec<_>>();

        if opt.deep_lock {
            if pass_list.len() != 3 {
                return Err("expected 3 newline delimited passphrases".into());
            }
            (
                pass_list.remove(0),
                pass_list.remove(0),
                Some(pass_list.remove(0)),
            )
        } else {
            if pass_list.len() != 2 {
                return Err("expected 2 newline delimited passphrases".into());
            }
            (pass_list.remove(0), pass_list.remove(0), None)
        }
    } else {
        (
            read_interactive_passphrase("\n# store passphrase> ").await?,
            read_interactive_passphrase("\n# bundle passphrase> ").await?,
            if opt.deep_lock {
                Some(
                    read_interactive_passphrase("\n# deep-lock passphrase> ")
                        .await?,
                )
            } else {
                None
            },
        )
    };

    // unlock the server
    server.run(store_pass).await?;

    // load the bundle
    let bundle_bytes =
        base64::decode_config(&opt.seed_bundle_base64, base64::URL_SAFE_NO_PAD)
            .map_err(one_err::OneErr::new)?;
    let cipher_list =
        hc_seed_bundle::UnlockedSeedBundle::from_locked(&bundle_bytes).await?;

    // attempt to unlock the bundle
    let mut bundle = None;
    for cipher in cipher_list {
        use hc_seed_bundle::LockedSeedCipher::*;
        match cipher {
            PwHash(pw_hash) => {
                match pw_hash.unlock(bundle_pass.clone()).await {
                    Ok(b) => {
                        bundle = Some(b);
                        break;
                    }
                    _ => continue,
                }
            }
            _ => continue,
        }
    }

    let bundle = bundle
        .ok_or_else(|| one_err::OneErr::from("could not unlock seed bundle"))?;

    // grab the seed from the unlocked bundle
    let seed = bundle.get_seed();

    // get the store from the server
    let store = server.store().await?;

    // insert the seed depending on deep lock or not
    let seed_info = if let Some(deep_pass) = deep_pass {
        let limits = hc_seed_bundle::PwHashLimits::Moderate;
        let ops_limit = limits.as_ops_limit();
        let mem_limit = limits.as_mem_limit();

        // pre-hash the passphrase
        let pw_hash = <sodoken::BufWriteSized<64>>::new_mem_locked()?;
        sodoken::hash::blake2b::hash(pw_hash.clone(), deep_pass).await?;

        store
            .insert_deep_locked_seed(
                seed,
                opt.tag.as_str().into(),
                ops_limit,
                mem_limit,
                pw_hash.to_read_sized(),
                opt.exportable,
            )
            .await?
    } else {
        store
            .insert_seed(seed, opt.tag.as_str().into(), opt.exportable)
            .await?
    };

    println!("# imported seed {} {:?}", opt.tag, seed_info);

    Ok(())
}
